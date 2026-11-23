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
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ActionConf {
    pub action: String,
    pub params: Params,
}

impl Default for ActionConf {
    fn default() -> Self {
        Self { action: "lock".into(), params: Params::default() }
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
    pub panic: bool,
}

impl Default for Notifications {
    fn default() -> Self {
        Self { toggle: true, reset: true, panic: false }
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
        Self { enabled: true, hotkey: String::new(), actions: vec![ActionConf::default()] }
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
        Self { enabled: true, grace: 30.0, actions: vec![ActionConf::default()] }
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
        Self { enabled: false, only_when_absent: true, grace: 0.0, actions: vec![ActionConf::default()] }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(default)]
pub struct Triggers {
    pub absence: Absence,
    pub stranger: Stranger,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Config {
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
    pub panic: Panic,
    pub triggers: Triggers,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            camera_index: 0,
            camera_name: String::new(),
            check_interval: 2.0,
            match_threshold: 0.363,
            capture_size: [640, 480],
            detect_size: [320, 240],
            stranger_cooldown: 20.0,
            hotkeys: Hotkeys::default(),
            notifications: Notifications::default(),
            panic: Panic::default(),
            triggers: Triggers::default(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
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
        let mut cfg: Config = std::fs::read(config_file())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        cfg.sanitize();
        cfg
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
