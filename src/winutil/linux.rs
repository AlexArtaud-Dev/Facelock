//! Utilitaires Linux côté interface. Les raccourcis sont saisis directement
//! dans la fenêtre (voir l'interface) : pas de capture clavier globale ici.

use super::ProcEntry;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Applications en cours (noms uniques, triés). Les processus noyau et
/// système sans exécutable lisible sont écartés.
pub fn list_processes() -> Vec<ProcEntry> {
    let uid = unsafe { libc::getuid() };
    let mut seen = BTreeMap::new();
    for (pid, name) in crate::actions::processes() {
        // Uniquement les processus de l'utilisateur courant
        let owner = std::fs::metadata(format!("/proc/{pid}")).map(|m| std::os::unix::fs::MetadataExt::uid(&m));
        if owner.ok() != Some(uid) {
            continue;
        }
        seen.entry(name.to_lowercase()).or_insert(name);
    }
    seen.into_values().map(|name| ProcEntry { name, icon: None }).collect()
}

/// Non utilisé sous Linux : la saisie se fait dans la fenêtre.
#[allow(dead_code)]
pub fn record_combo() -> String {
    String::new()
}

pub fn cancel_record() {}

/// Super/Windows : non visible via egui ; les raccourcis X11 l'acceptent
/// mais on ne le capture pas dans la fenêtre.
pub fn win_key_down() -> bool {
    false
}

fn autostart_file() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("autostart").join("facelock.desktop")
}

pub fn autostart_enabled() -> bool {
    let exe = std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    std::fs::read_to_string(autostart_file()).is_ok_and(|s| !exe.is_empty() && s.contains(&exe))
}

pub fn set_autostart(on: bool) -> bool {
    let path = autostart_file();
    if !on {
        let r = std::fs::remove_file(&path);
        return r.is_ok() || r.is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
    }
    let Ok(exe) = std::env::current_exe() else { return false };
    let content = format!(
        "[Desktop Entry]\nType=Application\nName=FaceLock\nComment=Verrouillage automatique par reconnaissance faciale\n\
         Exec=\"{}\"\nIcon=facelock\nTerminal=false\nX-GNOME-Autostart-enabled=true\n",
        exe.display()
    );
    path.parent().is_some_and(|d| std::fs::create_dir_all(d).is_ok()) && std::fs::write(&path, content).is_ok()
}

pub fn open_folder(path: &std::path::Path) {
    let _ = std::process::Command::new("xdg-open").arg(path).spawn();
}
