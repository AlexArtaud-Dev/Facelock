//! Actions Windows : appels Win32 directs (aucun processus lancé).

use super::{no_window_msg, pids_of};
use std::collections::HashSet;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LUID, WPARAM};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES,
    TOKEN_PRIVILEGES,
};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
};
use windows::Win32::System::Power::SetSuspendState;
use windows::Win32::System::Shutdown::LockWorkStation;
use windows::Win32::System::Threading::{
    GetCurrentProcess, OpenProcess, OpenProcessToken, TerminateProcess, PROCESS_TERMINATE,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_D, VK_LWIN,
    VK_MEDIA_PLAY_PAUSE, VK_VOLUME_MUTE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

type Log<'a> = &'a dyn Fn(String);

/// Simule une séquence de touches (code, relâchée ?).
fn send_keys(keys: &[(VIRTUAL_KEY, bool)]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: *vk,
                    dwFlags: if *up { KEYEVENTF_KEYUP } else { Default::default() },
                    ..Default::default()
                },
            },
        })
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn lock(log: Log) {
    if unsafe { LockWorkStation() }.is_err() {
        log("[erreur] Verrouillage impossible".into());
    }
}

pub fn sleep(log: Log) {
    unsafe {
        enable_shutdown_privilege();
        if !SetSuspendState(false, true, false) {
            log("[erreur] Mise en veille refusée par Windows".into());
        }
    }
}

pub fn switch_desktop(n: u32, log: Log) {
    match winvd::switch_desktop(n - 1) {
        Ok(()) => unsafe {
            // Après un changement programmatique, plus aucune fenêtre n'a
            // le focus clavier (Alt-Tab inerte) : on le rend au shell.
            std::thread::sleep(std::time::Duration::from_millis(150));
            let shell = GetShellWindow();
            if !shell.is_invalid() {
                SwitchToThisWindow(shell, true);
            }
        },
        Err(e) => log(format!("[erreur] Changement vers le bureau {n} impossible : {e:?}")),
    }
}

pub fn show_desktop(_log: Log) {
    send_keys(&[(VK_LWIN, false), (VK_D, false), (VK_D, true), (VK_LWIN, true)]);
}

pub fn media_pause(_log: Log) {
    send_keys(&[(VK_MEDIA_PLAY_PAUSE, false), (VK_MEDIA_PLAY_PAUSE, true)]);
}

pub fn mute(_log: Log) {
    send_keys(&[(VK_VOLUME_MUTE, false), (VK_VOLUME_MUTE, true)]);
}

pub fn screen_off(_log: Log) {
    unsafe {
        // SC_MONITORPOWER, 2 = éteint. Post : ne bloque jamais.
        let _ = PostMessageW(
            Some(HWND_BROADCAST),
            WM_SYSCOMMAND,
            WPARAM(SC_MONITORPOWER as usize),
            LPARAM(2),
        );
    }
}

pub fn minimize_app(process: &str, title: &str, log: Log) {
    let hwnds = windows_of(process, title);
    if hwnds.is_empty() {
        return log(no_window_msg(process, title));
    }
    for h in &hwnds {
        unsafe {
            let _ = ShowWindowAsync(*h, SW_MINIMIZE);
        }
    }
    log(format!("{} fenêtre(s) de {process} minimisée(s)", hwnds.len()));
}

pub fn move_app(process: &str, title: &str, n: u32, log: Log) {
    let hwnds = windows_of(process, title);
    if hwnds.is_empty() {
        return log(no_window_msg(process, title));
    }
    let moved = hwnds
        .iter()
        .filter(|h| winvd::move_window_to_desktop(n - 1, &windows58::Win32::Foundation::HWND(h.0)).is_ok())
        .count();
    log(format!(
        "{moved} fenêtre(s) de {process} déplacée(s) vers le bureau {n}"
    ));
}

pub fn kill_app(process: &str, title: &str, log: Log) {
    if !title.is_empty() {
        // Filtre de titre : on ferme uniquement les fenêtres correspondantes.
        let hwnds = windows_of(process, title);
        if hwnds.is_empty() {
            return log(no_window_msg(process, title));
        }
        for h in &hwnds {
            unsafe {
                let _ = PostMessageW(Some(*h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        return log(format!(
            "{} fenêtre(s) de {process} fermée(s) (titre ~ « {title} »)",
            hwnds.len()
        ));
    }
    let pids = pids_of(process);
    if pids.is_empty() {
        return log(format!("Aucun processus {process} en cours"));
    }
    let mut killed = 0;
    for pid in pids {
        unsafe {
            if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, pid) {
                if TerminateProcess(h, 1).is_ok() {
                    killed += 1;
                }
                let _ = CloseHandle(h);
            }
        }
    }
    if killed > 0 {
        log(format!("{killed} processus {process} terminé(s)"));
    } else {
        log(format!("[erreur] Impossible de terminer {process} (processus élevé ?)"));
    }
}

/// (pid, nom.exe) de tous les processus, via un instantané Toolhelp.
pub fn processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else {
            return out;
        };
        let mut e = PROCESSENTRY32W {
            dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
            ..Default::default()
        };
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
                out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..len])));
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
    }
    out
}

struct EnumCtx {
    pids: HashSet<u32>,
    needle: String,
    out: Vec<HWND>,
}

unsafe extern "system" fn enum_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let ctx = &mut *(lp.0 as *mut EnumCtx);
    if IsWindowVisible(hwnd).as_bool() {
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if ctx.pids.contains(&pid) && (ctx.needle.is_empty() || window_title(hwnd).to_lowercase().contains(&ctx.needle))
        {
            ctx.out.push(hwnd);
        }
    }
    BOOL(1)
}

fn window_title(hwnd: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(hwnd);
        if len <= 0 {
            return String::new();
        }
        let mut buf = vec![0u16; len as usize + 1];
        let n = GetWindowTextW(hwnd, &mut buf);
        String::from_utf16_lossy(&buf[..n.max(0) as usize])
    }
}

/// Fenêtres top-level visibles d'un processus, filtrées sur le titre.
fn windows_of(process: &str, title: &str) -> Vec<HWND> {
    let pids = pids_of(process);
    if pids.is_empty() {
        return Vec::new();
    }
    let mut ctx = EnumCtx {
        pids,
        needle: title.trim().to_lowercase(),
        out: Vec::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_cb), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.out
}

unsafe fn enable_shutdown_privilege() {
    let mut token = HANDLE::default();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES, &mut token).is_err() {
        return;
    }
    let mut luid = LUID::default();
    if LookupPrivilegeValueW(None, windows::core::w!("SeShutdownPrivilege"), &mut luid).is_ok() {
        let tp = TOKEN_PRIVILEGES {
            PrivilegeCount: 1,
            Privileges: [LUID_AND_ATTRIBUTES {
                Luid: luid,
                Attributes: SE_PRIVILEGE_ENABLED,
            }],
        };
        let _ = AdjustTokenPrivileges(token, false, Some(&tp), 0, None, None);
    }
    let _ = CloseHandle(token);
}
