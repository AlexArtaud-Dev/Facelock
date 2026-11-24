//! Configuration (config.json) - format compatible avec la version Python.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Params {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub process: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub desktop: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub text: String,
    /// Écran de confidentialité : image (chemin) à afficher en plein écran
    #[serde(skip_serializing_if = "String::is_empty")]
    pub image: String,
    /// Écran de confidentialité : raccourci de fermeture (obligatoire)
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hotkey: String,
    /// Écran de confidentialité : fermeture automatique quand un visage autorisé revient
    #[serde(skip_serializing_if = "Option::is_none")]
    pub close_on_return: Option<bool>,
}

impl Params {
    pub fn closes_on_return(&self) -> bool {
        self.close_on_return.unwrap_or(true)
    }
}

impl ActionConf {
    /// Problème bloquant de configuration (affiché dans l'éditeur, refusé à l'exécution).
    pub fn problem(&self) -> Option<&'static str> {
        match self.action.as_str() {
            "privacy_screen" if self.params.hotkey.trim().is_empty() => Some("raccourci de secours requis"),
            "privacy_screen" if crate::hotkey::parse_combo(&self.params.hotkey).is_err() => {
                Some("raccourci de secours invalide")
            }
            "kill_app" | "minimize_app" | "move_app" if self.params.process.trim().is_empty() => {
                Some("application à choisir")
            }
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ActionConf {
    pub action: String,
    pub params: Params,
}

impl Default for ActionConf {
    fn default() -> Self {
        Self {
            action: "lock".into(),
            params: Params::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Hotkeys {
    pub toggle: String,
    pub reset: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Notifications {
    pub toggle: bool,
    pub reset: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self {
            toggle: true,
            reset: true,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Panic {
    pub enabled: bool,
    pub hotkey: String,
    pub actions: Vec<ActionConf>,
}

impl Default for Panic {
    fn default() -> Self {
        Self {
            enabled: true,
            hotkey: String::new(),
            actions: vec![ActionConf::default()],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Absence {
    pub enabled: bool,
    pub grace: f64,
    pub actions: Vec<ActionConf>,
}

impl Default for Absence {
    fn default() -> Self {
        Self {
            enabled: true,
            grace: 30.0,
            actions: vec![ActionConf::default()],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Stranger {
    pub enabled: bool,
    pub only_when_absent: bool,
    pub grace: f64,
    pub actions: Vec<ActionConf>,
}

impl Default for Stranger {
    fn default() -> Self {
        Self {
            enabled: false,
            only_when_absent: true,
            grace: 0.0,
            actions: vec![ActionConf::default()],
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Triggers {
    pub absence: Absence,
    pub stranger: Stranger,
}

/// Quand le visage inconnu doit compter.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrangerWhen {
    /// Toujours
    #[default]
    Always,
    /// Seulement si aucun visage autorisé n'est là
    Alone,
    /// Seulement si un visage autorisé est là (regard par-dessus l'épaule)
    WithMe,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    /// Aucun visage autorisé depuis `grace` secondes
    Absence { grace: f64 },
    /// Visage inconnu présent depuis `grace` secondes
    Stranger { grace: f64, when: StrangerWhen },
    /// Retour d'un visage autorisé après au moins `min_absence` secondes
    Return { min_absence: f64 },
    /// Raccourci clavier global
    Hotkey { combo: String },
}

impl Trigger {
    pub fn kind(&self) -> &'static str {
        match self {
            Trigger::Absence { .. } => "absence",
            Trigger::Stranger { .. } => "stranger",
            Trigger::Return { .. } => "return",
            Trigger::Hotkey { .. } => "hotkey",
        }
    }

    pub fn default_of(kind: &str) -> Trigger {
        match kind {
            "stranger" => Trigger::Stranger {
                grace: 0.0,
                when: StrangerWhen::Always,
            },
            "return" => Trigger::Return { min_absence: 10.0 },
            "hotkey" => Trigger::Hotkey { combo: String::new() },
            _ => Trigger::Absence { grace: 30.0 },
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Workflow {
    pub id: u64,
    pub name: String,
    pub enabled: bool,
    pub trigger: Trigger,
    pub actions: Vec<ActionConf>,
    /// Délai minimal entre deux exécutions (secondes)
    pub cooldown: f64,
}

impl Default for Workflow {
    fn default() -> Self {
        Self {
            id: new_id(),
            name: "Nouveau workflow".into(),
            enabled: true,
            trigger: Trigger::Absence { grace: 30.0 },
            actions: vec![ActionConf::default()],
            cooldown: 20.0,
        }
    }
}

/// Identifiant unique (horloge + compteur).
pub fn new_id() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as u64)
        .unwrap_or(0);
    t.wrapping_add(N.fetch_add(1, Ordering::Relaxed))
}

pub fn action(id: &str) -> ActionConf {
    ActionConf {
        action: id.into(),
        params: Params::default(),
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
    /// 2 = workflows (v1 : triggers + panic)
    pub version: u32,
    pub camera_index: usize,
    /// Nom de la webcam : prioritaire sur l'index si elle est retrouvée.
    pub camera_name: String,
    pub check_interval: f64,
    pub match_threshold: f32,
    pub capture_size: [u32; 2],
    pub detect_size: [u32; 2],
    pub stranger_cooldown: f64,
    pub hotkeys: Hotkeys,
    pub notifications: Notifications,
    pub workflows: Vec<Workflow>,
    /// Format v1, lu uniquement pour la migration.
    #[serde(skip_serializing)]
    pub panic: Option<Panic>,
    #[serde(skip_serializing)]
    pub triggers: Option<Triggers>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            version: 2,
            camera_index: 0,
            camera_name: String::new(),
            check_interval: 2.0,
            match_threshold: 0.363,
            capture_size: [640, 480],
            detect_size: [320, 240],
            stranger_cooldown: 20.0,
            hotkeys: Hotkeys::default(),
            notifications: Notifications::default(),
            workflows: vec![Workflow {
                name: "Verrouillage automatique".into(),
                ..Workflow::default()
            }],
            panic: None,
            triggers: None,
        }
    }
}

/// %APPDATA%\FaceLock (Windows) ou $XDG_CONFIG_HOME/facelock (Linux).
pub fn data_dir() -> PathBuf {
    #[cfg(windows)]
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("FaceLock");
    #[cfg(not(windows))]
    let dir = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("facelock");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn faces_dir() -> PathBuf {
    let dir = data_dir().join("faces");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn config_file() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Self {
        let exists = config_file().exists();
        let mut cfg = std::fs::read(config_file())
            .map(|b| Config::parse(&b))
            .unwrap_or_default();
        let migrate = cfg.version < 2;
        if migrate && exists {
            // Copie de sécurité de l'ancien format avant réécriture.
            let _ = std::fs::copy(config_file(), data_dir().join("config.v1.json"));
        }
        if migrate {
            cfg.migrate_v1();
        }
        cfg.sanitize();
        // Les identifiants de workflow doivent être stables entre le service
        // et l'interface : on fige tout de suite le résultat de la migration.
        if migrate || !exists {
            let _ = cfg.save();
        }
        cfg
    }

    /// Lecture tolérante : JSON invalide -> défauts ; sans champ `version` -> format v1.
    pub fn parse(bytes: &[u8]) -> Config {
        serde_json::from_slice::<Config>(bytes)
            .map(|mut c| {
                if !bytes.windows(9).any(|w| w == b"\"version\"") {
                    c.version = 1;
                }
                c
            })
            .unwrap_or_default()
    }

    /// v1 (absence / inconnu / panic) -> trois workflows équivalents.
    fn migrate_v1(&mut self) {
        let t = self.triggers.take().unwrap_or_default();
        let p = self.panic.take().unwrap_or_default();
        self.workflows = vec![
            Workflow {
                name: "Verrouillage automatique".into(),
                enabled: t.absence.enabled,
                trigger: Trigger::Absence { grace: t.absence.grace },
                actions: t.absence.actions,
                ..Workflow::default()
            },
            Workflow {
                name: "Visage inconnu".into(),
                enabled: t.stranger.enabled,
                trigger: Trigger::Stranger {
                    grace: t.stranger.grace,
                    when: if t.stranger.only_when_absent {
                        StrangerWhen::Alone
                    } else {
                        StrangerWhen::Always
                    },
                },
                actions: t.stranger.actions,
                cooldown: self.stranger_cooldown,
                ..Workflow::default()
            },
            Workflow {
                name: "Panic button".into(),
                enabled: p.enabled && !p.hotkey.is_empty(),
                trigger: Trigger::Hotkey { combo: p.hotkey },
                actions: p.actions,
                cooldown: 0.0,
                ..Workflow::default()
            },
        ];
        self.version = 2;
    }

    pub fn sanitize(&mut self) {
        self.check_interval = self.check_interval.clamp(0.1, 60.0);
        self.match_threshold = self.match_threshold.clamp(0.0, 1.0);
        if self.capture_size[0] < 160 || self.capture_size[1] < 120 {
            self.capture_size = [640, 480];
        }
        if self.detect_size[0] < 96 {
            self.detect_size = [320, 240];
        }
        let mut seen = std::collections::HashSet::new();
        for w in &mut self.workflows {
            w.cooldown = w.cooldown.clamp(0.0, 3600.0);
            if w.id == 0 || !seen.insert(w.id) {
                w.id = new_id();
                seen.insert(w.id);
            }
        }
    }

    /// Écriture atomique (fichier temporaire + renommage).
    pub fn save(&self) -> std::io::Result<()> {
        let path = config_file();
        let tmp = path.with_extension("json.tmp");
        let json = serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, &path)
    }

    /// Tous les raccourcis configurés (non vides) et leur usage.
    pub fn hotkey_uses(&self) -> Vec<(HotkeyUse, String)> {
        let mut out = vec![
            (HotkeyUse::Toggle, self.hotkeys.toggle.clone()),
            (HotkeyUse::Reset, self.hotkeys.reset.clone()),
        ];
        for w in &self.workflows {
            if let Trigger::Hotkey { combo } = &w.trigger {
                out.push((HotkeyUse::Trigger(w.id), combo.clone()));
            }
            for (i, a) in w.actions.iter().enumerate() {
                if a.action == "privacy_screen" {
                    out.push((HotkeyUse::Dismiss(w.id, i), a.params.hotkey.clone()));
                }
            }
        }
        out.retain(|(_, c)| !c.trim().is_empty());
        out
    }

    /// Description lisible d'un usage, pour les messages d'erreur.
    pub fn describe(&self, u: &HotkeyUse) -> String {
        let name = |id: &u64| {
            self.workflows
                .iter()
                .find(|w| w.id == *id)
                .map(|w| w.name.as_str())
                .unwrap_or("?")
        };
        match u {
            HotkeyUse::Toggle => "le raccourci système « Pause / reprise »".into(),
            HotkeyUse::Reset => "le raccourci système « Réinitialiser »".into(),
            HotkeyUse::Trigger(id) => format!("le déclencheur du workflow « {} »", name(id)),
            HotkeyUse::Dismiss(id, _) => format!("le raccourci de secours du workflow « {} »", name(id)),
        }
    }

    /// Autre usage incompatible de la même combinaison (quelle que soit
    /// l'écriture : « alt+ctrl+p » = « ctrl+alt+p »). Plusieurs écrans de
    /// confidentialité peuvent partager le même raccourci de secours.
    pub fn hotkey_conflict(&self, combo: &str, me: &HotkeyUse) -> Option<HotkeyUse> {
        let key = crate::hotkey::parse_combo(combo).ok()?;
        self.hotkey_uses()
            .into_iter()
            .map(|(u, c)| (u, crate::hotkey::parse_combo(&c).ok()))
            .find_map(|(u, k)| {
                let both_dismiss = matches!((&u, me), (HotkeyUse::Dismiss(..), HotkeyUse::Dismiss(..)));
                (u != *me && k == Some(key) && !both_dismiss).then_some(u)
            })
    }
}

/// À quoi sert un raccourci (pour détecter les doublons).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum HotkeyUse {
    Toggle,
    Reset,
    /// Déclencheur « Raccourci clavier » d'un workflow
    Trigger(u64),
    /// Raccourci de secours d'un écran de confidentialité (workflow, action)
    Dismiss(u64, usize),
}

impl HotkeyUse {
    /// Workflow concerné, le cas échéant.
    pub fn workflow(&self) -> Option<u64> {
        match self {
            HotkeyUse::Trigger(id) | HotkeyUse::Dismiss(id, _) => Some(*id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_duplicate_hotkeys() {
        let mut c = Config::default();
        c.hotkeys.toggle = "ctrl+alt+p".into();
        let mut panic = Workflow {
            name: "Panic".into(),
            trigger: Trigger::Hotkey {
                combo: "ctrl+alt+w".into(),
            },
            ..Workflow::default()
        };
        let mut a = action("privacy_screen");
        a.params.hotkey = "ctrl+alt+shift+u".into();
        panic.actions = vec![a.clone()];
        let other = Workflow {
            name: "Autre".into(),
            actions: vec![a],
            ..Workflow::default()
        };
        let (pid, oid) = (panic.id, other.id);
        c.workflows = vec![panic, other];

        // Même combinaison écrite autrement que le raccourci système
        assert_eq!(
            c.hotkey_conflict("alt+ctrl+p", &HotkeyUse::Trigger(oid)),
            Some(HotkeyUse::Toggle)
        );
        // Le déclencheur de « Panic » est pris par un autre workflow
        assert_eq!(
            c.hotkey_conflict("ctrl+alt+w", &HotkeyUse::Trigger(oid)),
            Some(HotkeyUse::Trigger(pid))
        );
        // Réenregistrer la même combinaison au même endroit : pas de conflit
        assert_eq!(c.hotkey_conflict("ctrl+alt+w", &HotkeyUse::Trigger(pid)), None);
        // Deux écrans de confidentialité peuvent partager leur raccourci de secours…
        assert_eq!(c.hotkey_conflict("ctrl+alt+shift+u", &HotkeyUse::Dismiss(oid, 0)), None);
        // … mais pas avec un déclencheur
        assert!(matches!(
            c.hotkey_conflict("ctrl+alt+shift+u", &HotkeyUse::Trigger(oid)),
            Some(HotkeyUse::Dismiss(..))
        ));
        assert!(c.describe(&HotkeyUse::Trigger(pid)).contains("« Panic »"));
    }

    const V1: &str = r#"{
        "camera_index": 0, "check_interval": 0.5, "match_threshold": 0.363,
        "stranger_cooldown": 20.0,
        "hotkeys": { "toggle": "alt+ctrl+p", "reset": "ctrl+alt+r" },
        "panic": { "enabled": true, "hotkey": "ctrl+alt+w", "actions": [{ "action": "lock", "params": {} }] },
        "triggers": {
            "absence": { "enabled": false, "grace": 30.0, "actions": [{ "action": "lock", "params": {} }] },
            "stranger": { "enabled": true, "only_when_absent": false,
                          "actions": [{ "action": "switch_desktop", "params": { "desktop": 2 } }] }
        }
    }"#;

    #[test]
    fn migrates_v1_to_workflows() {
        let mut c = Config::parse(V1.as_bytes());
        assert_eq!(c.version, 1);
        c.migrate_v1();
        assert_eq!(c.version, 2);
        assert_eq!(c.check_interval, 0.5);
        assert_eq!(c.hotkeys.toggle, "alt+ctrl+p");
        let [abs, stranger, panic] = &c.workflows[..] else {
            panic!("3 workflows attendus")
        };
        assert!(!abs.enabled);
        assert_eq!(abs.trigger, Trigger::Absence { grace: 30.0 });
        assert!(stranger.enabled);
        assert_eq!(
            stranger.trigger,
            Trigger::Stranger {
                grace: 0.0,
                when: StrangerWhen::Always
            }
        );
        assert_eq!(stranger.actions[0].params.desktop, Some(2));
        assert_eq!(stranger.cooldown, 20.0);
        assert!(panic.enabled);
        assert_eq!(
            panic.trigger,
            Trigger::Hotkey {
                combo: "ctrl+alt+w".into()
            }
        );
        // Les anciens champs ne sont plus écrits
        let json = serde_json::to_string(&c).unwrap();
        assert!(!json.contains("\"triggers\"") && !json.contains("\"panic\""));
    }

    #[test]
    fn v2_roundtrip_and_garbage() {
        let c = Config::default();
        let back = Config::parse(&serde_json::to_vec(&c).unwrap());
        assert_eq!(back.version, 2);
        assert_eq!(back.workflows, c.workflows);
        assert_eq!(Config::parse(b"not json").version, 2);
    }
}
