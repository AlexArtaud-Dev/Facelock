//! Service résident : cycle de vie, IPC, raccourcis, lancement de l'interface.
//! La partie système (icône de notification, raccourcis globaux, suivi du
//! verrouillage) est fournie par `plat` (Windows ou Linux). Aucune
//! bibliothèque graphique n'est chargée dans ce processus.

use crate::config::{Config, Trigger};
use crate::hotkey;
use crate::ipc::{self, Cmd, FrameMeta, Reply, Req, Status};
use crate::state::{MonCmd, Shared};
use std::process::Child;
use std::sync::atomic::Ordering;
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
use linux as plat;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use self::windows as plat;

pub(crate) const HK_TOGGLE: i32 = 1;
pub(crate) const HK_RESET: i32 = 2;
pub(crate) const HK_WORKFLOW: i32 = 100;
/// Raccourcis de secours des écrans de confidentialité.
pub(crate) const HK_OVERLAY: i32 = 1000;

static SHARED: OnceLock<Arc<Shared>> = OnceLock::new();
static UI_CHILD: Mutex<Option<Child>> = Mutex::new(None);
/// Écran de confidentialité affiché : (processus, fermeture au retour du visage).
static OVERLAY: Mutex<Option<(Child, bool)>> = Mutex::new(None);
static APPLIED_HOTKEYS: Mutex<Vec<HotkeySpec>> = Mutex::new(Vec::new());

/// (id, combinaison, libellé, id du workflow éventuel)
pub(crate) type HotkeySpec = (i32, String, String, Option<u64>);

pub(crate) fn sh() -> &'static Arc<Shared> {
    SHARED.get().expect("état non initialisé")
}

/// Rafraîchit l'icône de notification (pause, caméra active…).
pub fn post_refresh(_sh: &Shared) {
    if SHARED.get().is_some() {
        plat::post_refresh();
    }
}

/// Notification système (titre « FaceLock »).
pub fn notify(message: &str) {
    if SHARED.get().is_some() {
        plat::notify(message);
    }
}

pub fn run() {
    if plat::already_running() {
        // Déjà lancé : on ouvre simplement la fenêtre de l'instance existante.
        plat::allow_foreground(None);
        ipc::send_once(Cmd::OpenUi { page: "live".into() });
        return;
    }

    let (tx, rx) = mpsc::channel();
    let shared = Shared::new(Config::load(), tx);
    let _ = SHARED.set(shared.clone());
    shared.log("FaceLock démarré");
    shared.locked.store(plat::session_locked(), Ordering::SeqCst);

    let mon_sh = shared.clone();
    let monitor = std::thread::Builder::new()
        .name("monitor".into())
        .stack_size(8 << 20)
        .spawn(move || crate::monitor::run(mon_sh, rx))
        .expect("thread moniteur");
    let ipc_sh = shared.clone();
    std::thread::Builder::new()
        .name("ipc".into())
        .spawn(move || ipc::serve(move |stream| client(ipc_sh.clone(), stream)))
        .expect("thread ipc");

    plat::run_loop(&shared);

    shared.log("Arrêt de FaceLock");
    shared.send(MonCmd::Quit);
    let _ = monitor.join();
    dismiss_overlay("arrêt du service");
    if let Some(mut c) = UI_CHILD.lock().unwrap().take() {
        let _ = c.kill();
    }
}

// ---------------------------------------------------------------- raccourcis

/// Calcule les raccourcis voulus et (ré)enregistre ceux qui ont changé.
/// `register(mods, vk, id)` / `unregister(id)` sont fournis par le système.
pub(crate) fn apply_hotkeys(
    force: bool,
    unregister: &mut dyn FnMut(i32),
    register: &mut dyn FnMut(u32, u32, i32) -> bool,
) {
    let s = sh();
    let cfg = s.cfg();
    let mut wanted: Vec<HotkeySpec> = Vec::new();
    if !s.hotkeys_paused.load(Ordering::SeqCst) {
        wanted.push((HK_TOGGLE, cfg.hotkeys.toggle.clone(), "pause / reprise".into(), None));
        wanted.push((HK_RESET, cfg.hotkeys.reset.clone(), "réinitialiser".into(), None));
        // Raccourcis de secours des écrans de confidentialité (un par combinaison).
        let mut seen = Vec::new();
        for a in cfg.workflows.iter().filter(|w| w.enabled).flat_map(|w| &w.actions) {
            let combo = a.params.hotkey.trim();
            if a.action == "privacy_screen" && !combo.is_empty() && !seen.contains(&combo.to_string()) {
                seen.push(combo.to_string());
                wanted.push((
                    HK_OVERLAY + seen.len() as i32,
                    combo.to_string(),
                    "fermer l'écran de confidentialité".into(),
                    None,
                ));
            }
        }
        for (i, wf) in cfg.workflows.iter().enumerate().filter(|(_, w)| w.enabled) {
            if let Trigger::Hotkey { combo } = &wf.trigger {
                wanted.push((
                    HK_WORKFLOW + i as i32,
                    combo.clone(),
                    format!("« {} »", wf.name),
                    Some(wf.id),
                ));
            }
        }
    }
    wanted.retain(|w| !w.1.trim().is_empty());
    let mut applied = APPLIED_HOTKEYS.lock().unwrap();
    if !force && *applied == wanted {
        return;
    }
    // Pause / reprise pendant l'enregistrement d'un raccourci : pas de bruit.
    let quiet = !force && (wanted.is_empty() || applied.is_empty());
    for (id, ..) in applied.iter() {
        unregister(*id);
    }
    let mut ok = Vec::new();
    for spec in &wanted {
        let (id, combo, name, _) = spec;
        match hotkey::parse_combo(combo) {
            Ok((mods, vk)) => {
                if register(mods, vk, *id) {
                    let was = applied.iter().any(|a| a.1 == *combo && a.2 == *name);
                    if !quiet && !was {
                        s.log(format!("Raccourci actif : {} → {name}", hotkey::pretty(combo)));
                    }
                    ok.push(spec.clone());
                } else {
                    s.log(format!(
                        "[erreur] Raccourci {} indisponible (déjà utilisé ailleurs ?)",
                        hotkey::pretty(combo)
                    ));
                }
            }
            Err(e) => s.log(format!("[erreur] Raccourci « {combo} » invalide : {e}")),
        }
    }
    *applied = ok;
}

pub(crate) fn on_hotkey(id: i32) {
    let s = sh();
    let cfg = s.cfg();
    if id >= HK_OVERLAY {
        dismiss_overlay("raccourci de secours");
        return;
    }
    if id >= HK_WORKFLOW {
        let wf_id = APPLIED_HOTKEYS
            .lock()
            .unwrap()
            .iter()
            .find(|a| a.0 == id)
            .and_then(|a| a.3);
        if let Some(wf) = wf_id.and_then(|w| cfg.workflows.iter().find(|x| x.id == w)) {
            crate::monitor::run_workflow(s, wf, Some("raccourci"));
        }
        return;
    }
    match id {
        HK_TOGGLE => {
            let now = !s.suspended.load(Ordering::SeqCst);
            s.set_suspended(now);
            if cfg.notifications.toggle {
                notify(if now {
                    "Surveillance suspendue"
                } else {
                    "Surveillance reprise"
                });
            }
        }
        HK_RESET => {
            s.send(MonCmd::Reset);
            if cfg.notifications.reset {
                notify("Vérification réinitialisée — le contrôle reprend de zéro");
            }
        }
        _ => {}
    }
}

// ----------------------------------------------------------------------- UI

// ------------------------------------------------- écran de confidentialité

/// Description transmise au processus d'affichage (`facelock --overlay <json>`).
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub struct OverlaySpec {
    pub text: String,
    pub image: String,
    pub hotkey: String,
}

/// Affiche l'écran de confidentialité dans un processus séparé. Le service
/// garde la main : son raccourci global (ou le retour du visage) le ferme,
/// même si la fenêtre ne répond plus.
pub fn show_overlay(p: &crate::config::Params, log: &dyn Fn(String)) {
    let mut cur = OVERLAY.lock().unwrap();
    if let Some((child, _)) = cur.as_mut() {
        if matches!(child.try_wait(), Ok(None)) {
            return; // déjà affiché
        }
    }
    let spec = OverlaySpec {
        text: p.text.trim().to_string(),
        image: p.image.trim().to_string(),
        hotkey: p.hotkey.clone(),
    };
    let json = serde_json::to_string(&spec).unwrap_or_default();
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["--overlay", &json]).spawn()) {
        Ok(child) => {
            plat::allow_foreground(Some(child.id()));
            log(format!(
                "Écran de confidentialité affiché (fermeture : {})",
                hotkey::pretty(&p.hotkey)
            ));
            *cur = Some((child, p.closes_on_return()));
        }
        Err(e) => log(format!("[erreur] Écran de confidentialité impossible : {e}")),
    }
}

pub fn dismiss_overlay(reason: &str) {
    if let Some((mut child, _)) = OVERLAY.lock().unwrap().take() {
        let running = matches!(child.try_wait(), Ok(None));
        let _ = child.kill();
        let _ = child.wait();
        if running {
            if let Some(s) = SHARED.get() {
                s.log(format!("Écran de confidentialité fermé ({reason})"));
            }
        }
    }
}

/// Appelé par le moniteur quand un visage autorisé est reconnu.
pub fn on_authorized_face() {
    let close = OVERLAY
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|(_, on_return)| *on_return);
    if close {
        dismiss_overlay("visage autorisé reconnu");
    }
}

pub(crate) fn open_ui(page: &str) {
    let s = sh();
    let mut child = UI_CHILD.lock().unwrap();
    if let Some(c) = child.as_mut() {
        if matches!(c.try_wait(), Ok(None)) {
            *s.nav.lock().unwrap() = Some(page.to_string());
            plat::allow_foreground(Some(c.id()));
            return;
        }
    }
    match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).args(["--ui", page]).spawn()) {
        Ok(c) => {
            plat::allow_foreground(Some(c.id()));
            *child = Some(c);
        }
        Err(e) => s.log(format!("[erreur] Ouverture de l'interface impossible : {e}")),
    }
}

// ---------------------------------------------------------------------- IPC

fn client(s: Arc<Shared>, mut f: ipc::Stream) {
    while let Ok((req, _)) = ipc::read_msg::<_, Req>(&mut f) {
        let res = match req {
            Req::Poll {
                log_since,
                want_frame,
                frame_seq,
            } => {
                let mut bin = Arc::new(Vec::new());
                let mut meta = None;
                if want_frame {
                    let was = s.frame_wanted();
                    *s.frame_wanted.lock().unwrap() = Some(Instant::now());
                    if !was {
                        s.send(MonCmd::Wake);
                    }
                    if let Some(p) = s.frame.lock().unwrap().as_ref() {
                        if p.seq != frame_seq {
                            bin = p.bgra.clone();
                            meta = Some(FrameMeta {
                                seq: p.seq,
                                w: p.w,
                                h: p.h,
                                faces: p.faces.clone(),
                            });
                        }
                    }
                }
                let (logs, log_next) = s.logs_since(log_since);
                let status = Status {
                    suspended: s.suspended.load(Ordering::SeqCst),
                    camera_active: s.camera_active.load(Ordering::SeqCst),
                    locked: s.locked.load(Ordering::SeqCst),
                    profiles: s.profiles.load(Ordering::Relaxed) as usize,
                    enroll: s.enroll.lock().unwrap().clone(),
                    logs,
                    log_next,
                    frame: meta,
                    nav: s.nav.lock().unwrap().take(),
                };
                ipc::write_msg(
                    &mut f,
                    &Reply {
                        ok: true,
                        msg: None,
                        status: Some(status),
                    },
                    &bin,
                )
            }
            Req::Cmd { cmd } => {
                let (ok, msg) = handle_cmd(&s, cmd);
                ipc::write_msg(&mut f, &Reply { ok, msg, status: None }, &[])
            }
        };
        if res.is_err() {
            break;
        }
    }
}

fn handle_cmd(s: &Arc<Shared>, cmd: Cmd) -> (bool, Option<String>) {
    match cmd {
        Cmd::ToggleSuspend => s.set_suspended(!s.suspended.load(Ordering::SeqCst)),
        Cmd::Reset => s.send(MonCmd::Reset),
        Cmd::ReloadConfig => {
            *s.cfg.write().unwrap() = Config::load();
            s.send(MonCmd::Wake);
            plat::post_hotkeys();
        }
        Cmd::ReloadProfiles => s.send(MonCmd::ReloadProfiles),
        Cmd::EnrollStart => s.send(MonCmd::EnrollStart),
        Cmd::EnrollCapture => s.send(MonCmd::EnrollCapture),
        Cmd::EnrollCancel => s.send(MonCmd::EnrollCancel),
        Cmd::EnrollSave { name } => {
            let (tx, rx) = mpsc::channel();
            s.send(MonCmd::EnrollSave(name, tx));
            return match rx.recv_timeout(Duration::from_secs(10)) {
                Ok(Ok(m)) => (true, Some(m)),
                Ok(Err(e)) => (false, Some(e)),
                Err(_) => (false, Some("Pas de réponse du moniteur".into())),
            };
        }
        Cmd::PauseHotkeys { paused } => {
            s.hotkeys_paused.store(paused, Ordering::SeqCst);
            plat::post_hotkeys();
        }
        Cmd::OpenUi { page } => open_ui(&page),
        Cmd::RunWorkflow { id } => {
            let cfg = s.cfg();
            match cfg.workflows.iter().find(|w| w.id == id) {
                Some(wf) => crate::monitor::run_workflow(s, wf, Some("test")),
                None => return (false, Some("Workflow introuvable (enregistrement en cours ?)".into())),
            }
        }
        Cmd::Quit => plat::quit(),
        Cmd::DismissOverlay => dismiss_overlay("ligne de commande"),
        Cmd::RunNamed { name } => {
            let cfg = s.cfg();
            let wanted = name.trim().to_lowercase();
            match cfg.workflows.iter().find(|w| w.name.trim().to_lowercase() == wanted) {
                Some(wf) => crate::monitor::run_workflow(s, wf, Some("ligne de commande")),
                None => return (false, Some(format!("Aucun workflow nommé « {name} »"))),
            }
        }
    }
    (true, None)
}
