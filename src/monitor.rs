//! Boucle de surveillance : caméra, reconnaissance, déclencheurs.
//!
//! Entièrement pilotée par événements : entre deux vérifications le thread
//! dort sur un canal (réveil instantané sur commande, verrouillage, etc.).
//! Session verrouillée / suspension / aucun profil : caméra libérée et
//! thread endormi indéfiniment, 0 % CPU.

use crate::actions;
use crate::camera::{self, Camera};
use crate::config::{Config, StrangerWhen, Trigger, Workflow};
use crate::engine::{box_iou, Engine, Face, Frame, EMB};
use crate::ipc::{EnrollInfo, FaceBox};
use crate::presence::{Presence, Silhouette};
use crate::state::{MonCmd, Published, Shared};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Un visage déjà identifié n'est ré-identifié (SFace) que s'il bouge
/// nettement ou après ce délai : la détection seule coûte ~10 ms.
const REVERIFY: Duration = Duration::from_secs(3);
const TRACK_IOU: f32 = 0.6;
const FAR: Duration = Duration::from_secs(3600);
const ENROLL_PERIOD: Duration = Duration::from_millis(66);
const MIN_SAMPLES: usize = 5;
/// Intervalle minimal entre deux détections de personnes.
const PEOPLE_PERIOD: Duration = Duration::from_millis(1500);
/// Idem quand l'aperçu caméra est affiché.
const PEOPLE_PERIOD_PREVIEW: Duration = Duration::from_millis(500);
/// Score minimal d'une silhouette (NanoDet, classe « person »).
const PERSON_SCORE: f32 = 0.35;

struct Tracked {
    bbox: [f32; 4],
    name: Option<String>,
    score: f32,
    at: Instant,
}

/// État d'exécution d'un workflow (indexé par son id).
#[derive(Default)]
struct WfState {
    since: Option<Instant>,
    fired: bool,
    last_fire: Option<Instant>,
}

struct Triggers {
    wf: HashMap<u64, WfState>,
    /// Un visage autorisé était-il là à la vérification précédente ?
    me_present: bool,
    absent_start: Option<Instant>,
}

impl Default for Triggers {
    fn default() -> Self {
        // Au démarrage / déverrouillage, l'utilisateur est supposé présent :
        // pas de faux « retour ».
        Triggers {
            wf: HashMap::new(),
            me_present: true,
            absent_start: None,
        }
    }
}

struct Enroll {
    embeddings: Vec<[f32; EMB]>,
    current: Option<(Frame, Face)>,
}

struct Mon {
    sh: Arc<Shared>,
    engine: Engine,
    cam: Option<Camera>,
    cam_key: (usize, String),
    last_read: Option<Instant>,
    trig: Triggers,
    tracked: Vec<Tracked>,
    enroll: Option<Enroll>,
    seq: u64,
    cam_error_logged: bool,
    /// Suivi des silhouettes (personnes au visage caché)
    presence: Presence,
    last_people: Option<Instant>,
    hidden: Vec<[f32; 4]>,
}

/// Exécute les actions d'un workflow dans un thread dédié (une action
/// « Attendre » ne bloque ni la surveillance ni les raccourcis).
pub fn run_workflow(sh: &Arc<Shared>, wf: &Workflow, origin: Option<&str>) {
    let sh = sh.clone();
    let wf = wf.clone();
    let origin = origin.map(str::to_string);
    std::thread::spawn(move || {
        // Raccourci : les actions partent avant toute journalisation.
        if origin.is_some() {
            actions::run_all(&wf.actions, &|m| sh.log(m));
        }
        sh.log(format!(
            "▶ « {} »{}",
            wf.name,
            origin.as_deref().map(|o| format!(" ({o})")).unwrap_or_default()
        ));
        if origin.is_none() {
            if wf.actions.is_empty() {
                sh.log("Aucune action configurée");
            }
            for a in &wf.actions {
                actions::run(a, &|m| sh.log(m));
            }
        }
    });
}

pub fn run(sh: Arc<Shared>, rx: Receiver<MonCmd>) {
    camera::init_thread();
    let engine = match Engine::new() {
        Ok(e) => e,
        Err(e) => {
            sh.log(format!("[erreur] Chargement des modèles impossible : {e}"));
            return;
        }
    };
    sh.profiles.store(engine.profiles.len() as u64, Ordering::Relaxed);
    // Les pages du modèle embarqué lues au chargement ne servent plus :
    // on les rend au système (working set réduit de ~40 Mo).
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessWorkingSetSize};
        let _ = SetProcessWorkingSetSize(GetCurrentProcess(), usize::MAX, usize::MAX);
    }
    if engine.profiles.is_empty() {
        sh.log("Aucun visage enrôlé — ouvre l'onglet Visages");
    }
    let mut m = Mon {
        sh,
        engine,
        cam: None,
        cam_key: (usize::MAX, String::new()),
        last_read: None,
        trig: Triggers::default(),
        tracked: Vec::new(),
        enroll: None,
        seq: 0,
        cam_error_logged: false,
        presence: Presence::default(),
        last_people: None,
        hidden: Vec::new(),
    };
    let mut next = Instant::now();
    loop {
        match rx.recv_timeout(next.saturating_duration_since(Instant::now())) {
            Ok(MonCmd::Quit) | Err(RecvTimeoutError::Disconnected) => break,
            Ok(cmd) => {
                m.command(cmd);
                next = Instant::now();
                continue;
            }
            Err(RecvTimeoutError::Timeout) => {}
        }
        let started = Instant::now();
        next = started + m.step();
    }
    m.release("arrêt");
}

impl Mon {
    fn command(&mut self, cmd: MonCmd) {
        match cmd {
            MonCmd::Wake | MonCmd::Quit => {}
            MonCmd::Reset => {
                self.trig = Triggers::default();
                self.tracked.clear();
                self.sh.suspended.store(false, Ordering::SeqCst);
                self.sh.log("Réinitialisation — la vérification reprend de zéro");
                crate::daemon::post_refresh(&self.sh);
            }
            MonCmd::ReloadProfiles => {
                self.engine.reload_profiles();
                self.tracked.clear();
                let n = self.engine.profiles.len();
                self.sh.profiles.store(n as u64, Ordering::Relaxed);
                self.sh.log(format!("{n} profil(s) chargé(s)"));
            }
            MonCmd::EnrollStart => {
                self.enroll = Some(Enroll {
                    embeddings: Vec::new(),
                    current: None,
                });
                *self.sh.enroll.lock().unwrap() = Some(EnrollInfo::default());
                *self.sh.frame_wanted.lock().unwrap() = Some(Instant::now());
                self.sh.log("Enrôlement démarré (surveillance en pause)");
            }
            MonCmd::EnrollCapture => {
                if let Some(en) = self.enroll.as_mut() {
                    if let Some((frame, face)) = &en.current {
                        match self.engine.embed(frame, face) {
                            Ok(e) => en.embeddings.push(e),
                            Err(e) => self.sh.log(format!("[erreur] Empreinte : {e}")),
                        }
                        if let Some(info) = self.sh.enroll.lock().unwrap().as_mut() {
                            info.count = en.embeddings.len();
                        }
                    }
                }
            }
            MonCmd::EnrollSave(name, reply) => {
                let name = name.trim().to_string();
                let res = match &self.enroll {
                    None => Err("Aucun enrôlement en cours".to_string()),
                    Some(_) if !crate::engine::valid_profile_name(&name) => {
                        Err("Nom invalide : lettres, chiffres, espaces, tirets ; 32 caractères max.".into())
                    }
                    Some(en) if en.embeddings.len() < MIN_SAMPLES => {
                        Err(format!("Au moins {MIN_SAMPLES} captures nécessaires"))
                    }
                    Some(en) => crate::engine::write_profile(&name, &en.embeddings)
                        .map(|_| format!("Profil « {name} » enregistré ({} empreintes)", en.embeddings.len()))
                        .map_err(|e| format!("Écriture impossible : {e}")),
                };
                if let Ok(msg) = &res {
                    self.sh.log(msg.clone());
                    self.end_enroll();
                    self.command(MonCmd::ReloadProfiles);
                }
                let _ = reply.send(res);
            }
            MonCmd::EnrollCancel => {
                if self.enroll.is_some() {
                    self.sh.log("Enrôlement annulé");
                }
                self.end_enroll();
            }
        }
    }

    fn end_enroll(&mut self) {
        self.enroll = None;
        *self.sh.enroll.lock().unwrap() = None;
        self.trig = Triggers::default();
        self.tracked.clear();
    }

    fn release(&mut self, reason: &str) {
        if self.cam.take().is_some() {
            self.sh.camera_active.store(false, Ordering::SeqCst);
            *self.sh.frame.lock().unwrap() = None;
            self.sh.log(format!("Caméra relâchée ({reason})"));
            crate::daemon::post_refresh(&self.sh);
        }
        self.last_read = None;
    }

    /// Annule les décomptes (verrouillage, suspension) en gardant l'anti-répétition.
    fn reset_absence(&mut self) {
        for st in self.trig.wf.values_mut() {
            st.since = None;
            st.fired = false;
        }
        self.trig.me_present = true;
        self.trig.absent_start = None;
        self.tracked.clear();
        self.presence.clear();
        self.hidden.clear();
        self.last_people = None;
    }

    fn ensure_camera(&mut self) -> bool {
        let cfg = self.sh.cfg();
        let key = (cfg.camera_index, cfg.camera_name.clone());
        if self.cam.is_some() && key == self.cam_key {
            return true;
        }
        if self.cam.is_some() {
            self.release("changement de caméra");
        }
        match Camera::open(cfg.camera_index, &cfg.camera_name, cfg.capture_size) {
            Ok(mut cam) => {
                // Laisse l'exposition automatique se stabiliser (~0,4 s).
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(400) {
                    if cam.read(false).is_err() {
                        break;
                    }
                }
                self.sh.log(format!(
                    "Caméra « {} » active ({}x{}) — surveillance en cours",
                    cam.name, cam.w, cam.h
                ));
                self.cam = Some(cam);
                self.cam_key = key;
                self.cam_error_logged = false;
                self.last_read = Some(Instant::now());
                self.sh.camera_active.store(true, Ordering::SeqCst);
                crate::daemon::post_refresh(&self.sh);
                true
            }
            Err(e) => {
                if !self.cam_error_logged {
                    self.sh
                        .log(format!("Caméra indisponible ({}), nouvel essai toutes les 10 s", e));
                    self.cam_error_logged = true;
                }
                false
            }
        }
    }

    fn grab(&mut self) -> Option<Frame> {
        let fresh = self.last_read.is_none_or(|t| t.elapsed() > Duration::from_millis(150));
        let res = self.cam.as_mut()?.read(fresh);
        self.last_read = Some(Instant::now());
        match res {
            Ok(f) => Some(f),
            Err(_) => {
                self.sh.log("Lecture caméra échouée, réinitialisation");
                self.release("erreur de lecture");
                None
            }
        }
    }

    fn publish(&mut self, frame: Frame, faces: Vec<FaceBox>) {
        let mut slot = self.sh.frame.lock().unwrap();
        if self.sh.frame_wanted() {
            self.seq += 1;
            *slot = Some(Published {
                seq: self.seq,
                w: frame.w,
                h: frame.h,
                bgra: Arc::new(frame.bgra),
                faces,
            });
        } else {
            *slot = None;
        }
    }

    /// Une itération ; renvoie le délai avant la suivante.
    fn step(&mut self) -> Duration {
        if self.enroll.is_some() {
            return self.enroll_step();
        }
        let cfg = self.sh.cfg();
        if self.sh.suspended.load(Ordering::SeqCst) {
            self.release("suspension");
            self.reset_absence();
            return FAR;
        }
        if self.sh.locked.load(Ordering::SeqCst) {
            self.release("session verrouillée");
            self.reset_absence();
            return FAR;
        }
        if self.engine.profiles.is_empty() {
            self.release("aucun visage enrôlé");
            return FAR;
        }
        if !self.ensure_camera() {
            return Duration::from_secs(10);
        }
        let Some(frame) = self.grab() else {
            return Duration::from_secs(2);
        };

        let faces = match self.engine.detect(&frame) {
            Ok(f) => f,
            Err(e) => {
                self.sh.log(format!("[erreur] Détection : {e}"));
                return Duration::from_secs_f64(cfg.check_interval);
            }
        };
        let now = Instant::now();
        let mut results: Vec<FaceBox> = Vec::with_capacity(faces.len());
        let mut tracked = Vec::with_capacity(faces.len());
        for face in &faces {
            let cached = self
                .tracked
                .iter()
                .find(|t| now.duration_since(t.at) < REVERIFY && box_iou(&t.bbox, &face.bbox) > TRACK_IOU);
            let (name, score, at) = match cached {
                Some(t) => (t.name.clone(), t.score, t.at),
                None => match self.engine.embed(&frame, face) {
                    Ok(emb) => {
                        let (n, s) = self.engine.identify(&emb, cfg.match_threshold);
                        (n, s, now)
                    }
                    Err(_) => continue,
                },
            };
            tracked.push(Tracked {
                bbox: face.bbox,
                name: name.clone(),
                score,
                at,
            });
            results.push(FaceBox {
                bbox: face.bbox,
                name,
                score,
                hidden: false,
                silhouette: 0,
            });
        }
        self.tracked = tracked;

        let me = results.iter().any(|r| r.name.is_some());
        if me {
            crate::daemon::on_authorized_face();
        }
        // Personnes au visage caché (seulement si un workflow « Visage
        // inconnu » est actif ou si l'aperçu est affiché ; au plus toutes les
        // PEOPLE_PERIOD : ~35 ms par analyse).
        let wanted = cfg.hidden_faces.enabled
            && (self.sh.frame_wanted()
                || cfg
                    .workflows
                    .iter()
                    .any(|w| w.enabled && matches!(w.trigger, Trigger::Stranger { .. })));
        // Aperçu ouvert : analyse plus fréquente pour un retour visuel réactif.
        let period = if self.sh.frame_wanted() {
            PEOPLE_PERIOD_PREVIEW
        } else {
            PEOPLE_PERIOD
        };
        if !wanted {
            self.hidden.clear();
            self.presence.view.clear();
        } else if self.last_people.is_none_or(|t| now.duration_since(t) >= period) {
            self.last_people = Some(now);
            match self.engine.detect_people(&frame, PERSON_SCORE) {
                Ok(people) => {
                    let boxes: Vec<[f32; 4]> = people.iter().map(|p| p.bbox).collect();
                    let faces: Vec<([f32; 4], bool)> = results.iter().map(|r| (r.bbox, r.name.is_some())).collect();
                    let hidden = self
                        .presence
                        .update(&boxes, &faces, frame.h as f32, cfg.hidden_faces.min_height, now);
                    if !hidden.is_empty() && self.hidden.is_empty() {
                        self.sh.log("Personne au visage caché détectée");
                    }
                    self.hidden = hidden;
                }
                Err(e) => self.sh.log(format!("[erreur] Détection de personnes : {e}")),
            }
        }
        let stranger = results.iter().any(|r| r.name.is_none()) || !self.hidden.is_empty();
        // Cadres de silhouettes pour l'aperçu (n'influencent pas les déclencheurs).
        if self.sh.frame_wanted() {
            for (b, state) in &self.presence.view {
                let code = match state {
                    Silhouette::FaceVisible => continue,
                    Silhouette::Me => 1,
                    Silhouette::Pending => 2,
                    Silhouette::Hidden => 3,
                    Silhouette::TooFar => 4,
                };
                results.push(FaceBox {
                    bbox: *b,
                    name: None,
                    score: 0.0,
                    hidden: code == 3,
                    silhouette: code,
                });
            }
        }
        self.publish(frame, results);
        self.triggers(me, stranger, now, &cfg);
        Duration::from_secs_f64(cfg.check_interval)
    }

    fn triggers(&mut self, me: bool, stranger: bool, now: Instant, cfg: &Config) {
        // Retour : transition « absent -> présent » après une absence suffisante.
        let back_after = if me && !self.trig.me_present {
            self.trig.absent_start.map(|t| now.duration_since(t))
        } else {
            None
        };
        if me {
            self.trig.absent_start = None;
        } else if self.trig.me_present || self.trig.absent_start.is_none() {
            self.trig.absent_start = Some(now);
        }
        self.trig.me_present = me;

        self.trig.wf.retain(|id, _| cfg.workflows.iter().any(|w| w.id == *id));
        for wf in cfg.workflows.iter().filter(|w| w.enabled) {
            let st = self.trig.wf.entry(wf.id).or_default();
            let cooled = st
                .last_fire
                .is_none_or(|t| now.duration_since(t).as_secs_f64() >= wf.cooldown);
            let fire = match &wf.trigger {
                Trigger::Absence { grace } => {
                    if me {
                        if st.since.take().is_some() && !st.fired && *grace > 0.0 {
                            self.sh
                                .log(format!("« {} » : visage reconnu, décompte annulé", wf.name));
                        }
                        st.fired = false;
                        false
                    } else {
                        let since = *st.since.get_or_insert_with(|| {
                            if *grace > 0.0 {
                                self.sh
                                    .log(format!("« {} » : absence détectée, décompte de {grace:.0} s", wf.name));
                            }
                            now
                        });
                        !st.fired && now.duration_since(since).as_secs_f64() >= *grace
                    }
                }
                Trigger::Stranger { grace, when } => {
                    let active = stranger
                        && match when {
                            StrangerWhen::Always => true,
                            StrangerWhen::Alone => !me,
                            StrangerWhen::WithMe => me,
                        };
                    if active {
                        let since = *st.since.get_or_insert(now);
                        now.duration_since(since).as_secs_f64() >= *grace && cooled
                    } else {
                        st.since = None;
                        false
                    }
                }
                Trigger::Return { min_absence } => {
                    back_after.is_some_and(|d| d.as_secs_f64() >= *min_absence) && cooled
                }
                Trigger::Hotkey { .. } => false,
            };
            if fire {
                st.fired = true;
                st.last_fire = Some(now);
                run_workflow(&self.sh, wf, None);
            }
        }
    }

    fn enroll_step(&mut self) -> Duration {
        // Interface fermée sans annuler : on n'immobilise pas la caméra.
        if !self.sh.frame_wanted() || self.sh.locked.load(Ordering::SeqCst) {
            self.sh.log("Enrôlement abandonné");
            self.end_enroll();
            return Duration::ZERO;
        }
        if !self.ensure_camera() {
            return Duration::from_secs(1);
        }
        let Some(frame) = self.grab() else {
            return Duration::from_millis(500);
        };
        let faces = self.engine.detect(&frame).unwrap_or_default();
        let one = faces.len() == 1;
        if let Some(info) = self.sh.enroll.lock().unwrap().as_mut() {
            info.face_ok = one;
            info.several = faces.len() > 1;
        }
        let boxes: Vec<FaceBox> = faces
            .iter()
            .map(|f| FaceBox {
                bbox: f.bbox,
                name: one.then(String::new),
                score: f.score,
                hidden: false,
                silhouette: 0,
            })
            .collect();
        // On garde la frame courante pour une éventuelle capture ; l'image
        // publiée est une copie (uniquement pendant l'enrôlement).
        let published = Frame {
            w: frame.w,
            h: frame.h,
            bgra: frame.bgra.clone(),
        };
        if let Some(en) = self.enroll.as_mut() {
            en.current = if one { Some((frame, faces[0].clone())) } else { None };
        }
        self.publish(published, boxes);
        ENROLL_PERIOD
    }
}
