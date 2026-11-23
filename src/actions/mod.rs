//! Actions exécutables par un workflow. Le catalogue et l'enchaînement sont
//! communs ; chaque système fournit l'implémentation (`sys`).

use crate::config::ActionConf;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as sys;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as sys;

pub use sys::processes;

/// Description d'un type d'action (pour l'éditeur visuel).
#[derive(Clone, Copy)]
pub struct Kind {
    pub id: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    /// Glyphe de la police d'icônes Phosphor
    pub icon: char,
    pub process: bool,
    pub desktop: bool,
    pub text: bool,
    pub seconds: bool,
    /// Écran de confidentialité (image, raccourci de secours…)
    #[allow(dead_code)]
    pub overlay: bool,
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
        overlay: false,
    }
}

pub const KINDS: &[Kind] = &[
    Kind {
        overlay: true,
        ..k(
            "privacy_screen",
            "Écran de confidentialité",
            "Masque tout l'écran avec un message ou une image",
            '\u{E224}',
        )
    },
    k(
        "lock",
        "Verrouiller la session",
        "Verrouillage instantané de la session",
        '\u{E2FA}',
    ),
    Kind {
        desktop: true,
        ..k(
            "switch_desktop",
            "Changer de bureau",
            "Bascule vers un bureau virtuel",
            '\u{E464}',
        )
    },
    k(
        "show_desktop",
        "Afficher le bureau",
        "Réduit toutes les fenêtres",
        '\u{E560}',
    ),
    Kind {
        process: true,
        ..k(
            "minimize_app",
            "Minimiser une appli",
            "Réduit les fenêtres d'une appli",
            '\u{E05C}',
        )
    },
    Kind {
        process: true,
        ..k(
            "kill_app",
            "Fermer une appli",
            "Ferme une appli ou certaines fenêtres",
            '\u{E4FA}',
        )
    },
    Kind {
        process: true,
        desktop: true,
        ..k(
            "move_app",
            "Déplacer une appli",
            "Envoie une appli sur un autre bureau",
            '\u{E5DE}',
        )
    },
    k(
        "media_pause",
        "Lecture / pause",
        "Met en pause ou relance la lecture",
        '\u{E8BE}',
    ),
    k(
        "mute",
        "Couper le son",
        "Coupe ou rétablit le son (bascule)",
        '\u{E45A}',
    ),
    k("screen_off", "Éteindre l'écran", "Met les écrans en veille", '\u{E32E}'),
    k("sleep", "Mettre en veille", "Veille du PC", '\u{E330}'),
    Kind {
        text: true,
        ..k("notify", "Notification", "Affiche une notification système", '\u{E0CE}')
    },
    Kind {
        seconds: true,
        ..k("wait", "Attendre", "Pause avant l'action suivante", '\u{E492}')
    },
];

pub fn kind(id: &str) -> Kind {
    KINDS
        .iter()
        .copied()
        .find(|k| k.id == id)
        .unwrap_or(k("?", "Action inconnue", "", '\u{E2CE}'))
}

pub fn run_all(actions: &[ActionConf], log: &dyn Fn(String)) {
    for a in actions {
        run(a, log);
    }
}

pub fn run(a: &ActionConf, log: &dyn Fn(String)) {
    if let Some(problem) = a.problem() {
        return log(format!("[erreur] {} : {problem}", kind(&a.action).label));
    }
    let p = &a.params;
    let process = p.process.trim();
    let title = p.title.trim();
    match a.action.as_str() {
        "privacy_screen" => crate::daemon::show_overlay(p, log),
        "lock" => sys::lock(log),
        "sleep" => sys::sleep(log),
        "switch_desktop" => sys::switch_desktop(p.desktop.unwrap_or(1).max(1), log),
        "show_desktop" => sys::show_desktop(log),
        "media_pause" => sys::media_pause(log),
        "mute" => sys::mute(log),
        "screen_off" => sys::screen_off(log),
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
        "kill_app" => sys::kill_app(process, title, log),
        "minimize_app" => sys::minimize_app(process, title, log),
        "move_app" => sys::move_app(process, title, p.desktop.unwrap_or(2).max(1), log),
        other => log(format!("[erreur] Action inconnue : {other}")),
    }
}

pub(crate) fn no_window_msg(process: &str, title: &str) -> String {
    if title.is_empty() {
        format!("Aucune fenêtre visible pour {process}")
    } else {
        format!("Aucune fenêtre visible pour {process} (titre ~ « {title} »)")
    }
}

/// PIDs d'un exécutable (nom insensible à la casse).
pub fn pids_of(process: &str) -> std::collections::HashSet<u32> {
    let target = process.to_lowercase();
    processes()
        .into_iter()
        .filter(|(_, n)| n.to_lowercase() == target)
        .map(|(p, _)| p)
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::config::{action, ActionConf};

    #[test]
    fn privacy_screen_requires_a_valid_hotkey() {
        let mut a: ActionConf = action("privacy_screen");
        assert_eq!(a.problem(), Some("raccourci de secours requis"));
        a.params.hotkey = "a+b".into();
        assert_eq!(a.problem(), Some("raccourci de secours invalide"));
        a.params.hotkey = "ctrl+alt+shift+u".into();
        assert_eq!(a.problem(), None);
        assert!(a.params.closes_on_return());
    }
}
