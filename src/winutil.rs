//! Utilitaires Windows côté interface : icônes de processus, enregistreur de
//! raccourcis (hook clavier temporaire), lancement à l'ouverture de session.

use crate::hotkey;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::time::{Duration, Instant};
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::FileSystem::FILE_FLAGS_AND_ATTRIBUTES;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Registry::*;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{SHGetFileInfoW, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const ICON: usize = 32;

pub struct ProcEntry {
    pub name: String,
    pub icon: Option<Vec<u8>>, // RGBA 32x32
}

fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            h,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        );
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Processus (.exe) uniques, triés, avec leur icône Explorateur.
pub fn list_processes() -> Vec<ProcEntry> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }
    let mut seen: BTreeMap<String, (String, Option<String>)> = BTreeMap::new();
    for (pid, name) in crate::actions::processes() {
        if pid == 0 || !name.to_lowercase().ends_with(".exe") {
            continue;
        }
        let entry = seen
            .entry(name.to_lowercase())
            .or_insert_with(|| (name.clone(), None));
        if entry.1.is_none() {
            entry.1 = exe_path(pid);
        }
    }
    seen.into_values()
        .map(|(name, path)| ProcEntry {
            icon: path.and_then(|p| file_icon(&p)),
            name,
        })
        .collect()
}

fn file_icon(path: &str) -> Option<Vec<u8>> {
    let wpath: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
    unsafe {
        let mut info = SHFILEINFOW::default();
        let r = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            FILE_FLAGS_AND_ATTRIBUTES(0),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if r == 0 || info.hIcon.is_invalid() {
            return None;
        }
        let out = hicon_rgba(info.hIcon);
        let _ = DestroyIcon(info.hIcon);
        out
    }
}

unsafe fn hicon_rgba(icon: HICON) -> Option<Vec<u8>> {
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(Some(screen));
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: ICON as i32,
            biHeight: -(ICON as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits = std::ptr::null_mut();
    let mut out = None;
    if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) {
        let old = SelectObject(mem, bmp.into());
        let _ = DrawIconEx(
            mem,
            0,
            0,
            icon,
            ICON as i32,
            ICON as i32,
            0,
            None,
            DI_NORMAL,
        );
        let _ = GdiFlush();
        let src = std::slice::from_raw_parts(bits as *const u8, ICON * ICON * 4);
        let no_alpha = src.chunks_exact(4).all(|p| p[3] == 0);
        let mut rgba = Vec::with_capacity(src.len());
        for p in src.chunks_exact(4) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], if no_alpha { 255 } else { p[3] }]);
        }
        out = Some(rgba);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
    }
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    out
}

// ------------------------------------------------------ enregistreur

const CAPTURE_WINDOW: Duration = Duration::from_millis(600);
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(8);

thread_local! {
    static PRESSED: RefCell<(Vec<u32>, Option<Instant>)> = const { RefCell::new((Vec::new(), None)) };
}

unsafe extern "system" fn ll_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        if wp.0 as u32 == WM_KEYDOWN || wp.0 as u32 == WM_SYSKEYDOWN {
            PRESSED.with(|p| {
                let mut p = p.borrow_mut();
                if p.1.is_none() {
                    p.1 = Some(Instant::now());
                }
                if !p.0.contains(&kb.vkCode) {
                    p.0.push(kb.vkCode);
                }
            });
        }
        // Touches avalées pendant la capture : le combo n'agit pas ailleurs.
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Capture bloquante d'une combinaison (à lancer dans un thread).
/// Renvoie "" si rien n'a été pressé, sinon la forme canonique "ctrl+alt+p".
pub fn record_combo() -> String {
    PRESSED.with(|p| *p.borrow_mut() = (Vec::new(), None));
    unsafe {
        let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), None, 0) else {
            return String::new();
        };
        let start = Instant::now();
        let timer = SetTimer(None, 0, 30, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let first = PRESSED.with(|p| p.borrow().1);
            if first.is_some_and(|t| t.elapsed() >= CAPTURE_WINDOW)
                || start.elapsed() >= CAPTURE_TIMEOUT
            {
                break;
            }
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(None, timer);
        let _ = UnhookWindowsHookEx(hook);
    }
    let keys = PRESSED.with(|p| p.borrow().0.clone());
    let mut mods = 0;
    let mut main = None;
    for vk in keys {
        match hotkey::vk_modifier(vk) {
            Some(m) => mods |= m,
            None if main.is_none() => main = Some(vk),
            None => {}
        }
    }
    match main {
        Some(vk) => hotkey::format_combo(mods, vk),
        None => String::new(),
    }
}

// -------------------------------------------------- démarrage auto

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("FaceLock");

/// Vrai si l'entrée de démarrage pointe vers CET exécutable
/// (une ancienne installation peut utiliser le même nom).
pub fn autostart_enabled() -> bool {
    unsafe {
        let mut buf = [0u16; 1024];
        let mut size = (buf.len() * 2) as u32;
        if RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            RUN_VALUE,
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut _),
            Some(&mut size),
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let len = (size as usize / 2).saturating_sub(1);
        let value = String::from_utf16_lossy(&buf[..len]).to_lowercase();
        let exe = std::env::current_exe()
            .map(|p| p.display().to_string().to_lowercase())
            .unwrap_or_default();
        !exe.is_empty() && value.contains(&exe)
    }
}

pub fn set_autostart(on: bool) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS
        {
            return false;
        }
        let ok = if on {
            let exe = std::env::current_exe()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            let val: Vec<u16> = format!("\"{exe}\"").encode_utf16().chain(Some(0)).collect();
            let bytes = std::slice::from_raw_parts(val.as_ptr() as *const u8, val.len() * 2);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes)) == ERROR_SUCCESS
        } else {
            let r = RegDeleteValueW(key, RUN_VALUE);
            r == ERROR_SUCCESS || r == ERROR_FILE_NOT_FOUND
        };
        let _ = RegCloseKey(key);
        ok
    }
}
