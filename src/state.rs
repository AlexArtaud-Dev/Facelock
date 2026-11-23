//! État partagé du daemon (tray, moniteur, serveur IPC).

use crate::config::Config;
use crate::ipc::{EnrollInfo, FaceBox};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;

pub enum MonCmd {
    Wake,
    Reset,
    ReloadProfiles,
    EnrollStart,
    EnrollCapture,
    EnrollSave(String, std::sync::mpsc::Sender<Result<String, String>>),
    EnrollCancel,
    Quit,
}

pub struct Published {
    pub seq: u64,
    pub w: usize,
    pub h: usize,
    pub bgra: Arc<Vec<u8>>,
    pub faces: Vec<FaceBox>,
}

const LOG_MAX: usize = 400;

pub struct Logs {
    lines: VecDeque<String>,
    next: u64,
}

pub struct Shared {
    pub cfg: RwLock<Config>,
    pub suspended: AtomicBool,
    pub camera_active: AtomicBool,
    pub locked: AtomicBool,
    pub hotkeys_paused: AtomicBool,
    pub profiles: AtomicU64,
    pub logs: Mutex<Logs>,
    pub frame: Mutex<Option<Published>>,
    /// Une interface a demandé l'image récemment (sinon : aucune copie).
    pub frame_wanted: Mutex<Option<Instant>>,
    pub enroll: Mutex<Option<EnrollInfo>>,
    pub nav: Mutex<Option<String>>,
    pub mon_tx: Mutex<Sender<MonCmd>>,
}

impl Shared {
    pub fn new(cfg: Config, mon_tx: Sender<MonCmd>) -> Arc<Self> {
        Arc::new(Shared {
            cfg: RwLock::new(cfg),
            suspended: AtomicBool::new(false),
            camera_active: AtomicBool::new(false),
            locked: AtomicBool::new(false),
            hotkeys_paused: AtomicBool::new(false),
            profiles: AtomicU64::new(0),
            logs: Mutex::new(Logs {
                lines: VecDeque::new(),
                next: 0,
            }),
            frame: Mutex::new(None),
            frame_wanted: Mutex::new(None),
            enroll: Mutex::new(None),
            nav: Mutex::new(None),
            mon_tx: Mutex::new(mon_tx),
        })
    }

    pub fn log(&self, msg: impl Into<String>) {
        let line = format!("[{}] {}", now_hms(), msg.into());
        let mut l = self.logs.lock().unwrap();
        l.lines.push_back(line);
        l.next += 1;
        while l.lines.len() > LOG_MAX {
            l.lines.pop_front();
        }
    }

    /// Lignes depuis l'index `since` (index global, croissant).
    pub fn logs_since(&self, since: u64) -> (Vec<String>, u64) {
        let l = self.logs.lock().unwrap();
        let first = l.next - l.lines.len() as u64;
        let skip = since.saturating_sub(first) as usize;
        (l.lines.iter().skip(skip).cloned().collect(), l.next)
    }

    pub fn send(&self, cmd: MonCmd) {
        let _ = self.mon_tx.lock().unwrap().send(cmd);
    }

    pub fn frame_wanted(&self) -> bool {
        self.frame_wanted
            .lock()
            .unwrap()
            .is_some_and(|t| t.elapsed().as_secs_f32() < 2.0)
    }

    pub fn cfg(&self) -> Config {
        self.cfg.read().unwrap().clone()
    }

    /// Verrouillage / déverrouillage de session (événement système).
    pub fn set_locked(&self, locked: bool) {
        if self.locked.swap(locked, Ordering::SeqCst) != locked && !locked {
            self.log("Session déverrouillée");
        }
        self.send(MonCmd::Wake);
    }

    pub fn set_suspended(&self, s: bool) {
        self.suspended.store(s, Ordering::SeqCst);
        self.log(if s {
            "Surveillance suspendue"
        } else {
            "Surveillance reprise"
        });
        self.send(MonCmd::Wake);
        crate::daemon::post_refresh(self);
    }
}

pub fn now_hms() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

/// Info-bulle de l'icône de notification.
pub fn tray_tip(s: &Shared) -> String {
    if s.suspended.load(Ordering::SeqCst) {
        "FaceLock — en pause"
    } else if s.camera_active.load(Ordering::SeqCst) {
        "FaceLock — protection active"
    } else {
        "FaceLock — en veille"
    }
    .to_string()
}
