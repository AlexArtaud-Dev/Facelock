//! Partie Windows du service : icône de notification Win32, RegisterHotKey
//! (aucun hook clavier), verrouillage suivi par événement (WTS).

use super::{apply_hotkeys, on_hotkey, open_ui, sh};
use crate::state::{MonCmd, Shared};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{WTSRegisterSessionNotification, NOTIFY_FOR_THIS_SESSION};
use windows::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_READOBJECTS,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT};
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const WM_TRAY: u32 = WM_APP + 1;
const WM_REFRESH: u32 = WM_APP + 2;
const WM_HOTKEYS: u32 = WM_APP + 3;

const ID_LIVE: u32 = 1;
const ID_WORKFLOWS: u32 = 2;
const ID_SUSPEND: u32 = 3;
const ID_RESET: u32 = 4;
const ID_QUIT: u32 = 9;

static HWND_TRAY: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: OnceLock<u32> = OnceLock::new();
static CURRENT_ICON: Mutex<isize> = Mutex::new(0);

fn hwnd() -> Option<HWND> {
    let h = HWND_TRAY.load(Ordering::SeqCst);
    (h != 0).then_some(HWND(h as *mut _))
}

fn post(msg: u32) {
    if let Some(h) = hwnd() {
        unsafe {
            let _ = PostMessageW(Some(h), msg, WPARAM(0), LPARAM(0));
        }
    }
}

pub fn post_refresh() {
    post(WM_REFRESH);
}

pub fn post_hotkeys() {
    post(WM_HOTKEYS);
}

pub fn quit() {
    post(WM_CLOSE);
}

pub fn already_running() -> bool {
    unsafe {
        let _ = CreateMutexW(None, true, w!("Local\\FaceLock.Daemon"));
        GetLastError() == ERROR_ALREADY_EXISTS
    }
}

pub fn allow_foreground(pid: Option<u32>) {
    unsafe {
        let _ = AllowSetForegroundWindow(pid.unwrap_or(ASFW_ANY));
    }
}

pub fn session_locked() -> bool {
    unsafe {
        match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_READOBJECTS) {
            Ok(h) => {
                let _ = CloseDesktop(h);
                false
            }
            Err(_) => true,
        }
    }
}

pub fn run_loop(_shared: &Arc<Shared>) {
    unsafe {
        let hinst = GetModuleHandleW(None).unwrap_or_default();
        let class = w!("FaceLockTray");
        let wc = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: hinst.into(),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let _ = TASKBAR_CREATED.set(RegisterWindowMessageW(w!("TaskbarCreated")));
        let Ok(h) = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class,
            w!("FaceLock"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst.into()),
            None,
        ) else {
            return;
        };
        HWND_TRAY.store(h.0 as isize, Ordering::SeqCst);
        tray_update(h, true);
        let _ = WTSRegisterSessionNotification(h, NOTIFY_FOR_THIS_SESSION);
        register_hotkeys(h, true);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        let _ = Shell_NotifyIconW(NIM_DELETE, &nid(h));
    }
}

fn register_hotkeys(h: HWND, force: bool) {
    apply_hotkeys(
        force,
        &mut |id| unsafe {
            let _ = UnregisterHotKey(Some(h), id);
        },
        &mut |mods, vk, id| unsafe { RegisterHotKey(Some(h), id, HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT, vk).is_ok() },
    );
}

// ------------------------------------------------------------------- tray

unsafe fn make_hicon(variant: crate::icon::Variant) -> HICON {
    let size = GetSystemMetrics(SM_CXSMICON).max(16) as usize;
    let rgba = crate::icon::rgba(size, variant);
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: size as i32,
            biHeight: -(size as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let Ok(color_bmp) = CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
        return HICON::default();
    };
    let dst = std::slice::from_raw_parts_mut(bits as *mut u8, size * size * 4);
    for (d, s) in dst.as_chunks_mut::<4>().0.iter_mut().zip(rgba.as_chunks::<4>().0) {
        *d = [s[2], s[1], s[0], s[3]];
    }
    let mask = CreateBitmap(size as i32, size as i32, 1, 1, None);
    let info = ICONINFO {
        fIcon: true.into(),
        hbmMask: mask,
        hbmColor: color_bmp,
        ..Default::default()
    };
    let icon = CreateIconIndirect(&info).unwrap_or_default();
    let _ = DeleteObject(color_bmp.into());
    let _ = DeleteObject(mask.into());
    icon
}

fn copy_wstr(dst: &mut [u16], s: &str) {
    let mut n = 0;
    for (d, c) in dst.iter_mut().zip(s.encode_utf16()) {
        *d = c;
        n += 1;
    }
    let n = n.min(dst.len() - 1);
    dst[n] = 0;
}

fn nid(h: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: h,
        uID: 1,
        ..Default::default()
    }
}

unsafe fn tray_update(h: HWND, add: bool) {
    let s = sh();
    let suspended = s.suspended.load(Ordering::SeqCst);
    let icon = make_hicon(if suspended {
        crate::icon::Variant::Suspended
    } else {
        crate::icon::Variant::Active
    });
    let mut d = nid(h);
    d.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    d.uCallbackMessage = WM_TRAY;
    d.hIcon = icon;
    copy_wstr(&mut d.szTip, &crate::state::tray_tip(s));
    let _ = Shell_NotifyIconW(if add { NIM_ADD } else { NIM_MODIFY }, &d);
    let mut cur = CURRENT_ICON.lock().unwrap();
    if *cur != 0 {
        let _ = DestroyIcon(HICON(*cur as *mut _));
    }
    *cur = icon.0 as isize;
}

/// Notification Windows (bulle / toast) via l'icône de notification.
pub fn notify(message: &str) {
    let Some(h) = hwnd() else { return };
    let mut d = nid(h);
    d.uFlags = NIF_INFO;
    copy_wstr(&mut d.szInfo, message);
    copy_wstr(&mut d.szInfoTitle, "FaceLock");
    d.dwInfoFlags = NIIF_INFO;
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }
}

unsafe fn show_menu(h: HWND) {
    let Ok(menu) = CreatePopupMenu() else { return };
    let suspended = sh().suspended.load(Ordering::SeqCst);
    let _ = AppendMenuW(menu, MF_STRING, ID_LIVE as usize, w!("Ouvrir FaceLock"));
    let _ = AppendMenuW(menu, MF_STRING, ID_WORKFLOWS as usize, w!("Workflows"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(
        menu,
        MF_STRING,
        ID_SUSPEND as usize,
        if suspended {
            w!("Reprendre")
        } else {
            w!("Mettre en pause")
        },
    );
    let _ = AppendMenuW(menu, MF_STRING, ID_RESET as usize, w!("Réinitialiser la vérification"));
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
    let _ = AppendMenuW(menu, MF_STRING, ID_QUIT as usize, w!("Quitter"));
    let _ = SetMenuDefaultItem(menu, ID_LIVE, 0);
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = SetForegroundWindow(h);
    let cmd = TrackPopupMenu(
        menu,
        TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
        pt.x,
        pt.y,
        None,
        h,
        None,
    );
    let _ = PostMessageW(Some(h), WM_NULL, WPARAM(0), LPARAM(0));
    let _ = DestroyMenu(menu);
    match cmd.0 as u32 {
        ID_LIVE => open_ui("live"),
        ID_WORKFLOWS => open_ui("workflows"),
        ID_SUSPEND => sh().set_suspended(!suspended),
        ID_RESET => sh().send(MonCmd::Reset),
        ID_QUIT => {
            let _ = DestroyWindow(h);
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(h: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    const WTS_SESSION_LOCK: usize = 0x7;
    const WTS_SESSION_UNLOCK: usize = 0x8;
    const PBT_APMRESUMEAUTOMATIC: usize = 0x12;
    match msg {
        WM_TRAY => {
            match lp.0 as u32 {
                WM_LBUTTONUP => open_ui("live"),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(h),
                _ => {}
            }
            LRESULT(0)
        }
        WM_HOTKEY => {
            on_hotkey(wp.0 as i32);
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wp.0 {
                WTS_SESSION_LOCK => sh().set_locked(true),
                WTS_SESSION_UNLOCK => sh().set_locked(false),
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            if wp.0 == PBT_APMRESUMEAUTOMATIC {
                sh().set_locked(session_locked());
            }
            LRESULT(1)
        }
        WM_REFRESH => {
            tray_update(h, false);
            LRESULT(0)
        }
        WM_HOTKEYS => {
            register_hotkeys(h, false);
            LRESULT(0)
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            LRESULT(0)
        }
        m if Some(&m) == TASKBAR_CREATED.get() => {
            // Explorer a redémarré : on remet l'icône.
            tray_update(h, true);
            LRESULT(0)
        }
        _ => DefWindowProcW(h, msg, wp, lp),
    }
}
