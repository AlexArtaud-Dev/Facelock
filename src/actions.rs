//! Actions déclenchées par FaceLock. Tout passe par des appels Win32 directs
//! (quelques microsecondes à quelques millisecondes, aucun processus lancé).

use crate::config::ActionConf;
use std::collections::HashSet;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HANDLE, HWND, LPARAM, LUID, WPARAM};
use windows::Win32::Security::{
    AdjustTokenPrivileges, LookupPrivilegeValueW, LUID_AND_ATTRIBUTES, SE_PRIVILEGE_ENABLED,
    TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES,
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
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_D,
    VK_LWIN, VK_MEDIA_PLAY_PAUSE, VK_VOLUME_MUTE,
};
use windows::Win32::UI::WindowsAndMessaging::*;

/// Description d'un type d'action (pour l'éditeur visuel).
#[derive(Clone, Copy)]
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    /// Glyphe Segoe Fluent Icons
    pub icon: char,
    pub process: bool,
    pub desktop: bool,
    pub text: bool,
    pub seconds: bool,
}

const fn k(id: &'static str, label: &'static str, desc: &'static str, icon: char) -> Kind {
    Kind {
        id,
        label,
        desc,
        icon,
        process: false,
        desktop: false,
        text: false,
        seconds: false,
    }
}

pub const KINDS: &[Kind] = &[
    k(
        "lock",
        "Verrouiller la session",
        "Comme Win+L, instantané",
        '\u{E72E}',
    ),
    Kind {
        desktop: true,
        ..k(
            "switch_desktop",
            "Changer de bureau",
            "Bascule vers un bureau virtuel",
            '\u{E7C4}',
        )
    },
    k(
        "show_desktop",
        "Afficher le bureau",
        "Réduit toutes les fenêtres",
        '\u{E8A9}',
    ),
    Kind {
        process: true,
        ..k(
            "minimize_app",
            "Minimiser une appli",
            "Réduit les fenêtres d'une appli",
            '\u{E921}',
        )
    },
    Kind {
        process: true,
        ..k(
            "kill_app",
            "Fermer une appli",
            "Ferme une appli ou certaines fenêtres",
            '\u{E711}',
        )
    },
    Kind {
        process: true,
        desktop: true,
        ..k(
            "move_app",
            "Déplacer une appli",
            "Envoie une appli sur un autre bureau",
            '\u{E8DE}',
        )
    },
    k(
        "media_pause",
        "Lecture / pause",
        "Touche multimédia lecture/pause",
        '\u{E769}',
    ),
    k(
        "mute",
        "Couper le son",
        "Touche multimédia muet (bascule)",
        '\u{E74F}',
    ),
    k(
        "screen_off",
        "Éteindre l'écran",
        "Met les écrans en veille",
        '\u{E7F4}',
    ),
    k("sleep", "Mettre en veille", "Veille du PC", '\u{E708}'),
    Kind {
        text: true,
        ..k(
            "notify",
            "Notification",
            "Affiche une notification Windows",
            '\u{EA8F}',
        )
    },
    Kind {
        seconds: true,
        ..k(
            "wait",
            "Attendre",
            "Pause avant l'action suivante",
            '\u{E916}',
        )
    },
];

pub fn kind(id: &str) -> Kind {
    KINDS
        .iter()
        .copied()
        .find(|k| k.id == id)
        .unwrap_or(k("?", "Action inconnue", "", '\u{E946}'))
}

/// Simule une séquence de touches (code, relâchée ?).
fn send_keys(keys: &[(VIRTUAL_KEY, bool)]) {
    let inputs: Vec<INPUT> = keys
        .iter()
        .map(|(vk, up)| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: *vk,
                    dwFlags: if *up {
                        KEYEVENTF_KEYUP
                    } else {
                        Default::default()
                    },
                    ..Default::default()
                },
            },
        })
        .collect();
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

pub fn run_all(actions: &[ActionConf], log: &dyn Fn(String)) {
    for a in actions {
        run(a, log);
    }
}

pub fn run(a: &ActionConf, log: &dyn Fn(String)) {
    let p = &a.params;
    let process = p.process.trim();
    let title = p.title.trim();
    match a.action.as_str() {
        "lock" => unsafe {
            if LockWorkStation().is_err() {
                log("[erreur] Verrouillage impossible".into());
            }
        },
        "sleep" => unsafe {
            enable_shutdown_privilege();
            if !SetSuspendState(false, true, false) {
                log("[erreur] Mise en veille refusée par Windows".into());
            }
        },
        "switch_desktop" => {
            let n = p.desktop.unwrap_or(1).max(1);
            match winvd::switch_desktop(n - 1) {
                Ok(()) => unsafe {
                    // Après un changement programmatique, plus aucune fenêtre n'a
                    // le focus clavier (Alt-Tab inerte) : on le rend au shell.
                    std::thread::sleep(std::time::Duration::from_millis(150));
                    let shell = GetShellWindow();
                    if !shell.is_invalid() {
                        windows::Win32::UI::WindowsAndMessaging::SwitchToThisWindow(shell, true);
                    }
                },
                Err(e) => log(format!(
                    "[erreur] Changement vers le bureau {n} impossible : {e:?}"
                )),
            }
        }
        "show_desktop" => send_keys(&[
            (VK_LWIN, false),
            (VK_D, false),
            (VK_D, true),
            (VK_LWIN, true),
        ]),
        "media_pause" => send_keys(&[(VK_MEDIA_PLAY_PAUSE, false), (VK_MEDIA_PLAY_PAUSE, true)]),
        "mute" => send_keys(&[(VK_VOLUME_MUTE, false), (VK_VOLUME_MUTE, true)]),
        "screen_off" => unsafe {
            // SC_MONITORPOWER, 2 = éteint. Post : ne bloque jamais.
            let _ = PostMessageW(
                Some(HWND_BROADCAST),
                WM_SYSCOMMAND,
                WPARAM(SC_MONITORPOWER as usize),
                LPARAM(2),
            );
        },
        "notify" => {
            let text = if p.text.trim().is_empty() {
                "Workflow déclenché"
            } else {
                p.text.trim()
            };
            crate::daemon::notify(text);
        }
        "wait" => {
            let secs = p.seconds.unwrap_or(1.0).clamp(0.0, 60.0);
            std::thread::sleep(std::time::Duration::from_secs_f64(secs));
        }
        "kill_app" | "minimize_app" | "move_app" if process.is_empty() => {
            log("[erreur] Aucun processus cible configuré".into());
        }
        "kill_app" => kill_app(process, title, log),
        "minimize_app" => {
            let hwnds = windows_of(process, title);
            if hwnds.is_empty() {
                log(no_window_msg(process, title));
                return;
            }
            for h in &hwnds {
                unsafe {
                    let _ = ShowWindowAsync(*h, SW_MINIMIZE);
                }
            }
            log(format!(
                "{} fenêtre(s) de {process} minimisée(s)",
                hwnds.len()
            ));
        }
        "move_app" => {
            let n = p.desktop.unwrap_or(2).max(1);
            let hwnds = windows_of(process, title);
            if hwnds.is_empty() {
                log(no_window_msg(process, title));
                return;
            }
            let moved = hwnds
                .iter()
                .filter(|h| {
                    winvd::move_window_to_desktop(n - 1, &windows58::Win32::Foundation::HWND(h.0))
                        .is_ok()
                })
                .count();
            log(format!(
                "{moved} fenêtre(s) de {process} déplacée(s) vers le bureau {n}"
            ));
        }
        other => log(format!("[erreur] Action inconnue : {other}")),
    }
}

fn no_window_msg(process: &str, title: &str) -> String {
    if title.is_empty() {
        format!("Aucune fenêtre visible pour {process}")
    } else {
        format!("Aucune fenêtre visible pour {process} (titre ~ « {title} »)")
    }
}

fn kill_app(process: &str, title: &str, log: &dyn Fn(String)) {
    if !title.is_empty() {
        // Filtre de titre : on ferme uniquement les fenêtres correspondantes.
        let hwnds = windows_of(process, title);
        if hwnds.is_empty() {
            log(no_window_msg(process, title));
            return;
        }
        for h in &hwnds {
            unsafe {
                let _ = PostMessageW(Some(*h), WM_CLOSE, WPARAM(0), LPARAM(0));
            }
        }
        log(format!(
            "{} fenêtre(s) de {process} fermée(s) (titre ~ « {title} »)",
            hwnds.len()
        ));
        return;
    }
    let pids = pids_of(process);
    if pids.is_empty() {
        log(format!("Aucun processus {process} en cours"));
        return;
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
        log(format!(
            "[erreur] Impossible de terminer {process} (processus élevé ?)"
        ));
    }
}

/// PIDs d'un exécutable (nom insensible à la casse) via un instantané Toolhelp.
pub fn pids_of(process: &str) -> HashSet<u32> {
    let target = process.to_lowercase();
    processes()
        .into_iter()
        .filter(|(_, n)| n.to_lowercase() == target)
        .map(|(p, _)| p)
        .collect()
}

/// (pid, nom.exe) de tous les processus.
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
                let len = e
                    .szExeFile
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(e.szExeFile.len());
                out.push((
                    e.th32ProcessID,
                    String::from_utf16_lossy(&e.szExeFile[..len]),
                ));
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
        if ctx.pids.contains(&pid)
            && (ctx.needle.is_empty() || window_title(hwnd).to_lowercase().contains(&ctx.needle))
        {
            ctx.out.push(hwnd);
        }
    }
    BOOL(1)
}

pub fn window_title(hwnd: HWND) -> String {
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
pub fn windows_of(process: &str, title: &str) -> Vec<HWND> {
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
