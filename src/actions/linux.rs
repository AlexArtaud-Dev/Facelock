//! Actions Linux : outils standard du bureau (loginctl, systemctl, xdotool,
//! wmctrl, pactl, playerctl, xset). Un outil absent est signalé dans le
//! journal avec le paquet à installer, sans bloquer la suite du workflow.

use super::{no_window_msg, pids_of};
use std::process::{Command, Stdio};

type Log<'a> = &'a dyn Fn(String);

/// Lance une commande et attend sa fin. Ok(true) = succès, Err = outil absent.
fn exec(cmd: &str, args: &[&str]) -> Result<bool, ()> {
    match Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
    {
        Ok(s) => Ok(s.success()),
        Err(_) => Err(()),
    }
}

fn output(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Essaie chaque commande dans l'ordre jusqu'à un succès.
fn first_ok(cmds: &[(&str, &[&str])], what: &str, hint: &str, log: Log) {
    let mut missing = true;
    for (cmd, args) in cmds {
        match exec(cmd, args) {
            Ok(true) => return,
            Ok(false) => missing = false,
            Err(()) => {}
        }
    }
    if missing {
        log(format!("[erreur] {what} : outil introuvable ({hint})"));
    } else {
        log(format!("[erreur] {what} : refusé par le système"));
    }
}

pub fn lock(log: Log) {
    first_ok(
        &[("loginctl", &["lock-session"]), ("xdg-screensaver", &["lock"])],
        "Verrouillage",
        "systemd ou xdg-utils",
        log,
    );
}

pub fn sleep(log: Log) {
    first_ok(&[("systemctl", &["suspend"])], "Mise en veille", "systemd", log);
}

pub fn switch_desktop(n: u32, log: Log) {
    let idx = (n - 1).to_string();
    first_ok(
        &[("wmctrl", &["-s", &idx]), ("xdotool", &["set_desktop", &idx])],
        &format!("Changement vers le bureau {n}"),
        "sudo apt install wmctrl xdotool",
        log,
    );
}

pub fn show_desktop(log: Log) {
    first_ok(
        &[("wmctrl", &["-k", "on"]), ("xdotool", &["key", "super+d"])],
        "Afficher le bureau",
        "sudo apt install wmctrl",
        log,
    );
}

pub fn media_pause(log: Log) {
    first_ok(
        &[("playerctl", &["play-pause"])],
        "Lecture / pause",
        "sudo apt install playerctl",
        log,
    );
}

pub fn mute(log: Log) {
    first_ok(
        &[
            ("pactl", &["set-sink-mute", "@DEFAULT_SINK@", "toggle"]),
            ("amixer", &["-q", "set", "Master", "toggle"]),
        ],
        "Couper le son",
        "pulseaudio-utils ou alsa-utils",
        log,
    );
}

pub fn screen_off(log: Log) {
    first_ok(
        &[("xset", &["dpms", "force", "off"])],
        "Éteindre l'écran",
        "sudo apt install x11-xserver-utils",
        log,
    );
}

/// Fenêtres visibles (identifiants X11) d'une application, filtrées sur le titre.
fn windows_of(process: &str, title: &str) -> Result<Vec<String>, ()> {
    let needle = title.trim().to_lowercase();
    let mut out = Vec::new();
    for pid in pids_of(process) {
        let list = match Command::new("xdotool")
            .args(["search", "--onlyvisible", "--pid", &pid.to_string()])
            .stderr(Stdio::null())
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).into_owned(),
            Err(_) => return Err(()),
        };
        for id in list.split_whitespace() {
            if needle.is_empty()
                || output("xdotool", &["getwindowname", id]).is_some_and(|t| t.to_lowercase().contains(&needle))
            {
                out.push(id.to_string());
            }
        }
    }
    Ok(out)
}

fn with_windows(process: &str, title: &str, log: Log, f: impl Fn(&str) -> bool) -> usize {
    match windows_of(process, title) {
        Err(()) => {
            log("[erreur] xdotool introuvable (sudo apt install xdotool)".into());
            0
        }
        Ok(ids) if ids.is_empty() => {
            log(no_window_msg(process, title));
            0
        }
        Ok(ids) => ids.iter().filter(|id| f(id)).count(),
    }
}

pub fn minimize_app(process: &str, title: &str, log: Log) {
    let n = with_windows(process, title, log, |id| {
        exec("xdotool", &["windowminimize", id]).unwrap_or(false)
    });
    if n > 0 {
        log(format!("{n} fenêtre(s) de {process} minimisée(s)"));
    }
}

pub fn move_app(process: &str, title: &str, desk: u32, log: Log) {
    let idx = (desk - 1).to_string();
    let n = with_windows(process, title, log, |id| {
        exec("xdotool", &["set_desktop_for_window", id, &idx]).unwrap_or(false)
    });
    if n > 0 {
        log(format!("{n} fenêtre(s) de {process} déplacée(s) vers le bureau {desk}"));
    }
}

pub fn kill_app(process: &str, title: &str, log: Log) {
    if !title.is_empty() {
        let n = with_windows(process, title, log, |id| {
            exec("xdotool", &["windowclose", id]).unwrap_or(false)
                || exec("wmctrl", &["-ic", &format!("0x{:x}", id.parse::<u64>().unwrap_or(0))]).unwrap_or(false)
        });
        if n > 0 {
            log(format!("{n} fenêtre(s) de {process} fermée(s) (titre ~ « {title} »)"));
        }
        return;
    }
    let pids = pids_of(process);
    if pids.is_empty() {
        return log(format!("Aucun processus {process} en cours"));
    }
    let killed = pids
        .iter()
        .filter(|&&pid| unsafe { libc::kill(pid as i32, libc::SIGTERM) } == 0)
        .count();
    if killed > 0 {
        log(format!("{killed} processus {process} terminé(s)"));
    } else {
        log(format!(
            "[erreur] Impossible de terminer {process} (autre utilisateur ?)"
        ));
    }
}

/// (pid, nom de l'exécutable) des processus visibles, via /proc.
pub fn processes() -> Vec<(u32, String)> {
    let mut out = Vec::new();
    let Ok(dir) = std::fs::read_dir("/proc") else {
        return out;
    };
    for e in dir.flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else {
            continue;
        };
        let name = std::fs::read_link(e.path().join("exe"))
            .ok()
            .and_then(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().trim_end_matches(" (deleted)").to_string())
            })
            .or_else(|| {
                std::fs::read_to_string(e.path().join("comm"))
                    .ok()
                    .map(|s| s.trim().to_string())
            });
        if let Some(n) = name.filter(|n| !n.is_empty()) {
            out.push((pid, n));
        }
    }
    out
}
