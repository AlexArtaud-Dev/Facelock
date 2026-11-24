//! Utilitaires Windows côté interface : icônes de processus, enregistreur de
//! raccourcis (hook clavier temporaire), lancement à l'ouverture de session.

use super::{ProcEntry, ICON};
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

pub fn open_folder(path: &std::path::Path) {
    let _ = std::process::Command::new("explorer").arg(path).spawn();
}

fn exe_path(pid: u32) -> Option<String> {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
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
        let entry = seen.entry(name.to_lowercase()).or_insert_with(|| (name.clone(), None));
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
        let _ = DrawIconEx(mem, 0, 0, icon, ICON as i32, ICON as i32, 0, None, DI_NORMAL);
        let _ = GdiFlush();
        let src = std::slice::from_raw_parts(bits as *const u8, ICON * ICON * 4);
        let px = src.as_chunks::<4>().0;
        let no_alpha = px.iter().all(|p| p[3] == 0);
        let mut rgba = Vec::with_capacity(src.len());
        for p in px {
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

const CAPTURE_TIMEOUT: Duration = Duration::from_secs(10);
/// Après la touche principale : délai max pour relâcher les modificateurs.
const RELEASE_GRACE: Duration = Duration::from_millis(1500);
const VK_ESCAPE: u32 = 0x1B;

#[derive(Default)]
struct Capture {
    /// Modificateurs actuellement enfoncés
    held: u32,
    /// (modificateurs, touche) capturés à l'appui de la touche principale
    result: Option<(u32, u32)>,
    done_at: Option<Instant>,
    cancelled: bool,
}

/// Demande d'arrêt de la capture (la saisie dans la fenêtre a répondu).
static CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn cancel_record() {
    CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// La fenêtre au premier plan appartient-elle à ce processus ?
fn foreground_is_us() -> bool {
    unsafe {
        let mut pid = 0u32;
        GetWindowThreadProcessId(GetForegroundWindow(), Some(&mut pid));
        pid == std::process::id()
    }
}

/// Touche Windows enfoncée (egui ne la remonte pas dans ses modificateurs).
pub fn win_key_down() -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LWIN, VK_RWIN};
    unsafe {
        (GetAsyncKeyState(VK_LWIN.0 as i32) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RWIN.0 as i32) as u16 & 0x8000) != 0
    }
}

thread_local! {
    static CAPTURE: RefCell<Capture> = RefCell::new(Capture::default());
}

unsafe extern "system" fn ll_proc(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 {
        let kb = &*(lp.0 as *const KBDLLHOOKSTRUCT);
        let down = matches!(wp.0 as u32, WM_KEYDOWN | WM_SYSKEYDOWN);
        CAPTURE.with(|c| {
            let mut c = c.borrow_mut();
            match hotkey::vk_modifier(kb.vkCode) {
                Some(m) if down => c.held |= m,
                Some(m) => c.held &= !m,
                // Touche principale : on fige la combinaison avec les
                // modificateurs maintenus à cet instant.
                None if down && c.result.is_none() && !c.cancelled => {
                    if kb.vkCode == VK_ESCAPE && c.held == 0 {
                        c.cancelled = true;
                    } else {
                        c.result = Some((c.held, kb.vkCode));
                    }
                    c.done_at = Some(Instant::now());
                }
                None => {}
            }
        });
        // Fenêtre FaceLock au premier plan : les touches lui parviennent
        // (c'est elle qui enregistre la combinaison).
        if foreground_is_us() {
            return CallNextHookEx(None, code, wp, lp);
        }
        // Ailleurs : touches avalées pendant la capture, la combinaison n'agit
        // nulle part (y compris les relâchements, pour ne pas ouvrir le menu
        // Démarrer avec un « Win » orphelin).
        return LRESULT(1);
    }
    CallNextHookEx(None, code, wp, lp)
}

/// Capture bloquante d'une combinaison (à lancer dans un thread).
/// Se termine dès qu'une touche principale est pressée (avec les
/// modificateurs maintenus), au rythme de l'utilisateur. Renvoie "" en cas
/// d'annulation (Échap) ou après 10 s sans touche principale.
pub fn record_combo() -> String {
    CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    CAPTURE.with(|c| *c.borrow_mut() = Capture::default());
    unsafe {
        let Ok(hook) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(ll_proc), None, 0) else {
            return String::new();
        };
        let start = Instant::now();
        let timer = SetTimer(None, 0, 20, None);
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let finished = CAPTURE.with(|c| {
                let c = c.borrow();
                // Fini quand tout est relâché (ou après un court délai).
                c.done_at.is_some_and(|t| c.held == 0 || t.elapsed() >= RELEASE_GRACE)
            });
            if finished || start.elapsed() >= CAPTURE_TIMEOUT || CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            DispatchMessageW(&msg);
        }
        let _ = KillTimer(None, timer);
        let _ = UnhookWindowsHookEx(hook);
    }
    CAPTURE
        .with(|c| c.borrow().result)
        .map(|(mods, vk)| hotkey::format_combo(mods, vk))
        .unwrap_or_default()
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
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key) != ERROR_SUCCESS {
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

#[cfg(test)]
mod tests {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;

    fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { Default::default() },
                    ..Default::default()
                },
            },
        }
    }

    /// Nécessite une session interactive (hook clavier bas niveau) : `cargo test -- --ignored`.
    /// Appui « humain » : modificateurs d'abord, la lettre près d'une seconde après.
    #[test]
    #[ignore]
    fn records_slow_combo() {
        let t = std::thread::spawn(super::record_combo);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let mods = [key(VK_CONTROL, false), key(VK_MENU, false), key(VK_SHIFT, false)];
        unsafe { SendInput(&mods, std::mem::size_of::<INPUT>() as i32) };
        std::thread::sleep(std::time::Duration::from_millis(900));
        let rest = [
            key(VIRTUAL_KEY(0x55), false),
            key(VIRTUAL_KEY(0x55), true),
            key(VK_SHIFT, true),
            key(VK_MENU, true),
            key(VK_CONTROL, true),
        ];
        unsafe { SendInput(&rest, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(t.join().unwrap(), "ctrl+alt+shift+u");
    }

    /// Un modificateur relâché avant la lettre ne fait pas partie du raccourci.
    #[test]
    #[ignore]
    fn ignores_released_modifier() {
        let t = std::thread::spawn(super::record_combo);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let seq = [
            key(VK_SHIFT, false),
            key(VK_SHIFT, true),
            key(VK_CONTROL, false),
            key(VK_F9, false),
            key(VK_F9, true),
            key(VK_CONTROL, true),
        ];
        unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(t.join().unwrap(), "ctrl+f9");
    }

    #[test]
    #[ignore]
    fn escape_cancels() {
        let t = std::thread::spawn(super::record_combo);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let seq = [key(VK_ESCAPE, false), key(VK_ESCAPE, true)];
        unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(t.join().unwrap(), "");
    }

    #[test]
    #[ignore]
    fn records_injected_combo() {
        let t = std::thread::spawn(super::record_combo);
        std::thread::sleep(std::time::Duration::from_millis(400));
        let seq = [
            key(VK_CONTROL, false),
            key(VK_MENU, false),
            key(VK_SHIFT, false),
            key(VIRTUAL_KEY(0x55), false),
            key(VIRTUAL_KEY(0x55), true),
            key(VK_SHIFT, true),
            key(VK_MENU, true),
            key(VK_CONTROL, true),
        ];
        unsafe { SendInput(&seq, std::mem::size_of::<INPUT>() as i32) };
        assert_eq!(t.join().unwrap(), "ctrl+alt+shift+u");
    }
}
