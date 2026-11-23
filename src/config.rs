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
            "hotkey" => Trigger::Hotkey {
                combo: String::new(),
            },
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

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = base.join("FaceLock");
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
                trigger: Trigger::Absence {
                    grace: t.absence.grace,
                },
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
}

#[cfg(test)]
mod tests {
    use super::*;

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
