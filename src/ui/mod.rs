//! Interface (processus séparé, lancé à la demande depuis l'icône).
//! Rendu immédiat egui : rien n'est redessiné tant que rien ne change ;
//! fermer la fenêtre termine le processus et libère toute sa mémoire.

mod flow;
pub mod overlay;
mod theme;

use crate::config::{Config, Trigger, Workflow};
use crate::hotkey;
use crate::ipc::{self, Cmd, FaceBox, FrameMeta, Reply, Req, Status};
use crate::winutil;
use eframe::egui::{self, Align, Align2, Color32, FontId, Layout, RichText, Sense, Stroke, StrokeKind, Vec2};
use flow::{Op, Sel};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use theme::*;

const TARGET_SAMPLES: usize = 12;
const MIN_SAMPLES: usize = 5;

pub fn run(page: &str) {
    let icon = egui::IconData {
        rgba: crate::icon::rgba(128, crate::icon::Variant::Active),
        width: 128,
        height: 128,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FaceLock")
            .with_inner_size([1120.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    let page = page.to_string();
    let _ = eframe::run_native("FaceLock", opts, Box::new(move |cc| Ok(Box::new(App::new(cc, &page)))));
}

// ------------------------------------------------------------------ lien

#[derive(Default)]
struct Link {
    connected: bool,
    status: Status,
    logs: Vec<String>,
    log_next: u64,
    frame: Option<(FrameMeta, Vec<u8>)>,
    frame_seq: u64,
    want_frame: bool,
    fast: bool,
    replies: Vec<Reply>,
}

fn link_thread(link: Arc<Mutex<Link>>, rx: Receiver<(Cmd, bool)>, ctx: egui::Context) {
    loop {
        let Some(mut pipe) = ipc::connect() else {
            {
                let mut l = link.lock().unwrap();
                if l.connected {
                    l.connected = false;
                    ctx.request_repaint();
                }
            }
            match rx.recv_timeout(Duration::from_secs(1)) {
                Err(RecvTimeoutError::Disconnected) => return,
                _ => continue,
            }
        };
        link.lock().unwrap().connected = true;
        ctx.request_repaint();
        loop {
            let (period, want_frame, log_since, frame_seq) = {
                let l = link.lock().unwrap();
                let p = if l.fast {
                    50
                } else if l.want_frame {
                    150
                } else {
                    250
                };
                (Duration::from_millis(p), l.want_frame, l.log_next, l.frame_seq)
            };
            match rx.recv_timeout(period) {
                Ok((cmd, want_reply)) => {
                    if ipc::write_msg(&mut pipe, &Req::Cmd { cmd }, &[]).is_err() {
                        break;
                    }
                    match ipc::read_msg::<_, Reply>(&mut pipe) {
                        Ok((r, _)) if want_reply || !r.ok => {
                            link.lock().unwrap().replies.push(r);
                            ctx.request_repaint();
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                    continue;
                }
                Err(RecvTimeoutError::Disconnected) => return,
                Err(RecvTimeoutError::Timeout) => {}
            }
            if ipc::write_msg(
                &mut pipe,
                &Req::Poll {
                    log_since,
                    want_frame,
                    frame_seq,
                },
                &[],
            )
            .is_err()
            {
                break;
            }
            let Ok((reply, bin)) = ipc::read_msg::<_, Reply>(&mut pipe) else {
                break;
            };
            let Some(st) = reply.status else { continue };
            let mut l = link.lock().unwrap();
            let changed = !st.logs.is_empty()
                || st.frame.is_some()
                || st.nav.is_some()
                || st.suspended != l.status.suspended
                || st.camera_active != l.status.camera_active
                || st.locked != l.status.locked
                || st.profiles != l.status.profiles
                || st.enroll != l.status.enroll;
            l.logs.extend(st.logs.iter().cloned());
            let excess = l.logs.len().saturating_sub(400);
            l.logs.drain(..excess);
            l.log_next = st.log_next;
            if let Some(meta) = st.frame.clone() {
                l.frame_seq = meta.seq;
                l.frame = Some((meta, bin));
            }
            let nav = st.nav.clone().or(l.status.nav.take());
            l.status = Status {
                logs: Vec::new(),
                frame: None,
                nav,
                ..st
            };
            drop(l);
            if changed {
                ctx.request_repaint();
            }
        }
        link.lock().unwrap().connected = false;
        ctx.request_repaint();
    }
}

// ------------------------------------------------------------------- app

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Dashboard,
    Workflows,
    Faces,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
enum Rec {
    Toggle,
    Reset,
    Workflow(u64),
    Action(u64, usize),
}

struct Picker {
    target: (u64, usize),
    search: String,
    rx: Option<Receiver<Vec<winutil::ProcEntry>>>,
    items: Vec<(String, Option<egui::TextureHandle>)>,
    focus: bool,
}

struct EnrollUi {
    name: String,
    error: Option<String>,
    saving: bool,
}

enum Confirm {
    Profile(String),
    Workflow(usize),
}

struct App {
    page: Page,
    link: Arc<Mutex<Link>>,
    tx: Sender<(Cmd, bool)>,
    cfg: Config,
    saved: Config,
    dirty_since: Option<Instant>,
    toast: Option<(String, Instant, bool)>,
    show_video: bool,
    tex: Option<egui::TextureHandle>,
    tex_seq: u64,
    faces: Vec<FaceBox>,
    frame_size: (usize, usize),
    cameras: Vec<String>,
    cam_rx: Option<Receiver<Vec<String>>>,
    profiles: Vec<String>,
    enroll: Option<EnrollUi>,
    confirm: Option<Confirm>,
    picker: Option<Picker>,
    gallery: bool,
    recording: Option<(Rec, Receiver<String>)>,
    autostart: bool,
    sel: Option<(u64, Sel)>,
    pending_test: Option<u64>,
    logo: Option<egui::TextureHandle>,
    /// Saisie de raccourci dans la fenêtre (Linux)
    rec_tx: Option<(Sender<String>, Instant)>,
}

fn page_from(name: &str) -> Page {
    match name {
        "settings" | "general" | "shortcuts" => Page::Settings,
        "faces" => Page::Faces,
        "workflows" | "triggers" => Page::Workflows,
        _ => Page::Dashboard,
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, page: &str) -> Self {
        theme::setup(&cc.egui_ctx);
        let link = Arc::new(Mutex::new(Link::default()));
        let (tx, rx) = mpsc::channel();
        {
            let (link, ctx) = (link.clone(), cc.egui_ctx.clone());
            std::thread::spawn(move || link_thread(link, rx, ctx));
        }
        let cfg = Config::load();
        let mut app = App {
            page: page_from(page),
            link,
            tx,
            saved: cfg.clone(),
            cfg,
            dirty_since: None,
            toast: None,
            show_video: true,
            tex: None,
            tex_seq: 0,
            faces: Vec::new(),
            frame_size: (640, 480),
            cameras: Vec::new(),
            cam_rx: None,
            profiles: Vec::new(),
            enroll: None,
            confirm: None,
            picker: None,
            gallery: false,
            recording: None,
            autostart: winutil::autostart_enabled(),
            sel: None,
            pending_test: None,
            logo: None,
            rec_tx: None,
        };
        app.refresh_cameras(&cc.egui_ctx);
        app.refresh_profiles();
        app
    }

    fn send(&self, cmd: Cmd) {
        let _ = self.tx.send((cmd, false));
    }

    fn toast(&mut self, msg: impl Into<String>, ok: bool) {
        self.toast = Some((msg.into(), Instant::now(), ok));
    }

    fn refresh_cameras(&mut self, ctx: &egui::Context) {
        let (tx, rx) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            crate::camera::init_thread();
            let _ = tx.send(crate::camera::list_cameras());
            ctx.request_repaint();
        });
        self.cam_rx = Some(rx);
    }

    fn refresh_profiles(&mut self) {
        self.profiles = crate::engine::list_profiles().into_iter().map(|(n, _)| n).collect();
    }

    fn save_now(&mut self) {
        self.cfg.sanitize();
        match self.cfg.save() {
            Ok(()) => {
                self.send(Cmd::ReloadConfig);
                self.toast("Modifications enregistrées", true);
            }
            Err(e) => self.toast(format!("Enregistrement impossible : {e}"), false),
        }
        self.saved = self.cfg.clone();
        self.dirty_since = None;
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if self.cfg == self.saved {
            self.dirty_since = None;
        } else {
            let since = *self.dirty_since.get_or_insert_with(Instant::now);
            let wait = Duration::from_millis(400);
            if since.elapsed() < wait {
                ctx.request_repaint_after(wait - since.elapsed());
            } else {
                self.save_now();
            }
        }
        // Test demandé : le service doit d'abord connaître la dernière version.
        if let Some(id) = self.pending_test.take() {
            if self.cfg != self.saved {
                self.save_now();
            }
            self.send(Cmd::RunWorkflow { id });
            let name = self
                .cfg
                .workflows
                .iter()
                .find(|w| w.id == id)
                .map(|w| w.name.clone())
                .unwrap_or_default();
            self.toast(format!("« {name} » exécuté"), true);
        }
    }

    /// Synchronise l'état reçu du service (navigation, image, réponses).
    fn pump(&mut self, ctx: &egui::Context) {
        let wants_video = (self.page == Page::Dashboard && self.show_video) || self.enroll.is_some();
        let mut l = self.link.lock().unwrap();
        l.want_frame = wants_video;
        l.fast = self.enroll.is_some();
        if let Some(nav) = l.status.nav.take() {
            if nav == "live" || self.page == Page::Dashboard {
                self.page = page_from(&nav);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if let Some((meta, bin)) = l.frame.take() {
            if meta.seq != self.tex_seq && bin.len() == meta.w * meta.h * 4 {
                let mut rgba = bin;
                for px in rgba.as_chunks_mut::<4>().0 {
                    px.swap(0, 2);
                    px[3] = 255;
                }
                let img = egui::ColorImage::from_rgba_unmultiplied([meta.w, meta.h], &rgba);
                match &mut self.tex {
                    Some(t) => t.set(img, egui::TextureOptions::LINEAR),
                    None => self.tex = Some(ctx.load_texture("camera", img, egui::TextureOptions::LINEAR)),
                }
                self.tex_seq = meta.seq;
                self.faces = meta.faces;
                self.frame_size = (meta.w, meta.h);
            }
        }
        let replies: Vec<Reply> = std::mem::take(&mut l.replies);
        drop(l);
        for r in replies {
            if let Some(en) = self.enroll.as_mut().filter(|e| e.saving) {
                en.saving = false;
                if r.ok {
                    let msg = r.msg.unwrap_or_else(|| "Profil enregistré".into());
                    self.enroll = None;
                    self.tex = None;
                    self.refresh_profiles();
                    self.toast(msg, true);
                } else {
                    en.error = r.msg;
                }
            } else if !r.ok {
                self.toast(r.msg.unwrap_or_else(|| "Erreur".into()), false);
            }
        }
        if let Some(rx) = &self.cam_rx {
            if let Ok(list) = rx.try_recv() {
                self.cameras = list;
                self.cam_rx = None;
            }
        }
        if let Some((target, rx)) = &self.recording {
            if let Ok(combo) = rx.try_recv() {
                let target = *target;
                self.recording = None;
                self.send(Cmd::PauseHotkeys { paused: false });
                if combo.is_empty() {
                    self.toast("Enregistrement annulé : aucune touche principale reçue", false);
                } else {
                    match hotkey::parse_combo(&combo) {
                        Ok(_) => {
                            self.toast(format!("Raccourci enregistré : {}", hotkey::pretty(&combo)), true);
                            self.set_combo(target, combo);
                        }
                        Err(e) => self.toast(format!("Raccourci refusé : {e}"), false),
                    }
                }
            }
        }
    }

    fn set_combo(&mut self, target: Rec, combo: String) {
        match target {
            Rec::Toggle => self.cfg.hotkeys.toggle = combo,
            Rec::Reset => self.cfg.hotkeys.reset = combo,
            Rec::Workflow(id) => {
                if let Some(Trigger::Hotkey { combo: c }) = self
                    .cfg
                    .workflows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .map(|w| &mut w.trigger)
                {
                    *c = combo;
                }
            }
            Rec::Action(id, i) => {
                if let Some(a) = self
                    .cfg
                    .workflows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .and_then(|w| w.actions.get_mut(i))
                {
                    a.params.hotkey = combo;
                }
            }
        }
    }

    /// Windows : capture globale (hook clavier temporaire, toutes les touches,
    /// Win comprise). Linux : saisie dans la fenêtre (voir `capture_in_window`).
    fn start_recording(&mut self, ctx: &egui::Context, target: Rec) {
        self.send(Cmd::PauseHotkeys { paused: true });
        let (tx, rx) = mpsc::channel();
        if cfg!(windows) {
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(winutil::record_combo());
                ctx.request_repaint();
            });
        } else {
            self.rec_tx = Some((tx, Instant::now()));
        }
        self.recording = Some((target, rx));
    }

    /// Saisie d'un raccourci au clavier dans la fenêtre (Linux).
    fn capture_in_window(&mut self, ctx: &egui::Context) {
        let Some((tx, since)) = &self.rec_tx else { return };
        if since.elapsed() > Duration::from_secs(10) {
            let _ = tx.send(String::new());
            self.rec_tx = None;
            return;
        }
        let combo = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let name = egui_key_name(*key)?;
                    let mut parts = Vec::new();
                    if modifiers.ctrl {
                        parts.push("ctrl".to_string());
                    }
                    if modifiers.alt {
                        parts.push("alt".to_string());
                    }
                    if modifiers.shift {
                        parts.push("shift".to_string());
                    }
                    parts.push(name);
                    Some(parts.join("+"))
                }
                _ => None,
            })
        });
        if let Some(c) = combo {
            let _ = tx.send(c);
            self.rec_tx = None;
        } else {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}

/// Nom de touche egui -> nom FaceLock (« a », « f5 », « page up »…).
pub(crate) fn egui_key_name(key: egui::Key) -> Option<String> {
    use egui::Key;
    let n = match key {
        Key::Space => "space",
        Key::Enter => "enter",
        Key::Escape => "esc",
        Key::Tab => "tab",
        Key::Backspace => "backspace",
        Key::Delete => "delete",
        Key::Insert => "insert",
        Key::Home => "home",
        Key::End => "end",
        Key::PageUp => "page up",
        Key::PageDown => "page down",
        Key::ArrowUp => "up",
        Key::ArrowDown => "down",
        Key::ArrowLeft => "left",
        Key::ArrowRight => "right",
        other => {
            let name = other.name().to_lowercase();
            // Lettres, chiffres, F1..F24
            let valid = (name.len() == 1 && name.chars().all(|c| c.is_ascii_alphanumeric()))
                || (name.starts_with('f') && name[1..].parse::<u32>().is_ok_and(|n| (1..=24).contains(&n)));
            return valid.then_some(name);
        }
    };
    Some(n.to_string())
}

impl Drop for App {
    fn drop(&mut self) {
        if self.enroll.is_some() {
            ipc::send_once(Cmd::EnrollCancel);
        }
        if self.recording.is_some() {
            ipc::send_once(Cmd::PauseHotkeys { paused: false });
        }
        if self.cfg != self.saved {
            let _ = self.cfg.save();
            ipc::send_once(Cmd::ReloadConfig);
        }
    }
}

// ----------------------------------------------------------------- statut

struct Look {
    color: Color32,
    icon: char,
    title: &'static str,
    sub: &'static str,
}

fn look(connected: bool, s: &Status) -> Look {
    let (color, icon, title, sub) = if !connected {
        (
            FAINT,
            I_POWER,
            "Service arrêté",
            "Lance FaceLock pour démarrer la surveillance.",
        )
    } else if s.enroll.is_some() {
        (
            ACCENT,
            I_FACE,
            "Enrôlement en cours",
            "La surveillance reprend à la fin de l'enrôlement.",
        )
    } else if s.suspended {
        (
            ORANGE,
            I_PAUSE,
            "Surveillance en pause",
            "La caméra est libérée jusqu'à la reprise.",
        )
    } else if s.locked {
        (
            MUTED,
            I_LOCK,
            "Session verrouillée",
            "Caméra coupée, aucune ressource utilisée.",
        )
    } else if s.profiles == 0 {
        (
            MUTED,
            I_FACE,
            "Aucun visage enrôlé",
            "Ajoute ton visage pour activer la protection.",
        )
    } else if s.camera_active {
        (
            GREEN,
            I_EYE,
            "Protection active",
            "Ton poste est surveillé, tes workflows sont prêts.",
        )
    } else {
        (
            ORANGE,
            I_VIDEO,
            "Caméra indisponible",
            "Vérifie la webcam dans les réglages.",
        )
    };
    Look {
        color,
        icon,
        title,
        sub,
    }
}

// ------------------------------------------------------------------ rendu

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        theme::backdrop(&ctx);
        self.capture_in_window(&ctx);
        self.pump(&ctx);
        let (connected, status) = {
            let l = self.link.lock().unwrap();
            (l.connected, l.status.clone())
        };

        egui::Panel::left("sidebar")
            .exact_size(232.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(SIDEBAR)
                    .stroke(Stroke::new(1.0, BORDER))
                    .inner_margin(egui::Margin {
                        left: 16,
                        right: 16,
                        top: 22,
                        bottom: 16,
                    }),
            )
            .show(ui, |ui| self.sidebar(ui, connected, &status));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().inner_margin(egui::Margin {
                left: 32,
                right: 32,
                top: 28,
                bottom: 8,
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.set_max_width(ui.available_width().min(1000.0));
                    match self.page {
                        Page::Dashboard => self.page_dashboard(ui, connected, &status),
                        Page::Workflows => self.page_workflows(ui),
                        Page::Faces => self.page_faces(ui, &status),
                        Page::Settings => self.page_settings(ui),
                    }
                    ui.add_space(24.0);
                });
            });

        self.gallery_modal(&ctx);
        self.picker_modal(&ctx);
        self.confirm_modal(&ctx);
        self.toast_overlay(&ctx);
        self.autosave(&ctx);
    }
}

impl App {
    fn sidebar(&mut self, ui: &mut egui::Ui, connected: bool, status: &Status) {
        // Logo
        ui.horizontal(|ui| {
            let logo = self.logo.get_or_insert_with(|| {
                let px = crate::icon::rgba(96, crate::icon::Variant::Active);
                ui.ctx().load_texture(
                    "logo",
                    egui::ColorImage::from_rgba_unmultiplied([96, 96], &px),
                    egui::TextureOptions::LINEAR,
                )
            });
            ui.image(egui::load::SizedTexture::new(logo.id(), Vec2::splat(42.0)));
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("FaceLock").family(semibold()).size(18.0));
                ui.label(small(concat!("v", env!("CARGO_PKG_VERSION"))));
            });
        });
        ui.add_space(26.0);

        let active = self.cfg.workflows.iter().filter(|w| w.enabled).count();
        let items = [
            (Page::Dashboard, I_DASH, "Tableau de bord", String::new()),
            (Page::Workflows, I_FLOW, "Workflows", active.to_string()),
            (Page::Faces, I_FACE, "Visages", self.profiles.len().to_string()),
            (Page::Settings, I_SETTINGS, "Réglages", String::new()),
        ];
        for (page, ic, label, count) in items {
            let selected = self.page == page;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 42.0), Sense::click());
            let p = ui.painter();
            if selected {
                p.rect_filled(rect, 10.0, SURFACE2);
                p.rect_filled(
                    egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 11.0), Vec2::new(3.0, 20.0)),
                    2.0,
                    ACCENT,
                );
            } else if resp.hovered() {
                p.rect_filled(rect, 10.0, SURFACE.gamma_multiply(0.9));
            }
            let fg = if selected { TEXT } else { MUTED };
            p.text(
                egui::pos2(rect.left() + 24.0, rect.center().y),
                Align2::CENTER_CENTER,
                ic,
                FontId::new(16.0, icons()),
                if selected { ACCENT } else { fg },
            );
            p.text(
                egui::pos2(rect.left() + 46.0, rect.center().y),
                Align2::LEFT_CENTER,
                label,
                FontId::new(
                    14.5,
                    if selected {
                        semibold()
                    } else {
                        egui::FontFamily::Proportional
                    },
                ),
                fg,
            );
            if !count.is_empty() {
                let g = p.layout_no_wrap(count, FontId::new(11.5, semibold()), MUTED);
                let r = egui::Rect::from_center_size(
                    egui::pos2(rect.right() - 20.0, rect.center().y),
                    g.size() + Vec2::new(12.0, 4.0),
                );
                p.rect_filled(r, 9.0, SURFACE3);
                p.galley(r.min + Vec2::new(6.0, 2.0), g, MUTED);
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.page = page;
            }
            ui.add_space(2.0);
        }

        // Statut en bas
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            let lk = look(connected, status);
            egui::Frame::new()
                .fill(SURFACE2)
                .corner_radius(14)
                .stroke(Stroke::new(1.0, BORDER))
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        status_dot(ui, lk.color);
                        ui.label(RichText::new(lk.title).size(13.0));
                    });
                    if connected {
                        let (txt, ic, kind) = if status.suspended {
                            ("Reprendre", I_PLAY, Kind::Tint(GREEN))
                        } else {
                            ("Mettre en pause", I_PAUSE, Kind::Ghost)
                        };
                        if button(ui, Some(ic), txt, kind, true).clicked() {
                            self.send(Cmd::ToggleSuspend);
                        }
                    }
                });
        });
    }

    // --------------------------------------------------------- tableau de bord

    fn page_dashboard(&mut self, ui: &mut egui::Ui, connected: bool, status: &Status) {
        let lk = look(connected, status);
        // Bandeau d'état
        let hero = glass_frame(24)
            .fill(lerp_color(SURFACE, lk.color.gamma_multiply(0.25), 0.35))
            .stroke(Stroke::new(1.0, lerp_color(BORDER, lk.color.gamma_multiply(0.5), 0.5)))
            .inner_margin(26)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(84.0), Sense::hover());
                    let p = ui.painter();
                    p.circle_filled(rect.center(), 42.0, lk.color.gamma_multiply(0.12));
                    p.circle_stroke(rect.center(), 34.0, Stroke::new(3.0, lk.color));
                    p.text(
                        rect.center(),
                        Align2::CENTER_CENTER,
                        lk.icon,
                        FontId::new(28.0, icons()),
                        lk.color,
                    );
                    ui.add_space(14.0);
                    ui.vertical(|ui| {
                        ui.add_space(10.0);
                        ui.label(h1(lk.title));
                        ui.label(muted(lk.sub));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if button(ui, Some(I_REFRESH), "Réinitialiser", Kind::Ghost, connected)
                            .on_hover_text("Annule les décomptes, lève la pause et relance la vérification")
                            .clicked()
                        {
                            self.send(Cmd::Reset);
                        }
                        let (txt, ic, kind) = if status.suspended {
                            ("Reprendre", I_PLAY, Kind::Primary)
                        } else {
                            ("Pause", I_PAUSE, Kind::Ghost)
                        };
                        if button(ui, Some(ic), txt, kind, connected).clicked() {
                            self.send(Cmd::ToggleSuspend);
                        }
                    });
                });
            });
        sheen(ui.painter(), hero.response.rect, 24.0);
        ui.add_space(14.0);

        // Indicateurs
        let active = self.cfg.workflows.iter().filter(|w| w.enabled).count();
        let tiles = [
            (I_FACE, "Visages enrôlés", status.profiles.to_string(), ACCENT),
            (
                I_FLOW,
                "Workflows actifs",
                format!("{active} / {}", self.cfg.workflows.len()),
                VIOLET,
            ),
            (
                I_CLOCK,
                "Vérification",
                format!("toutes les {:.1} s", self.cfg.check_interval).replace(".0 s", " s"),
                TEAL,
            ),
            (
                I_VIDEO,
                "Caméra",
                if status.camera_active {
                    "Active".into()
                } else {
                    "Coupée".into()
                },
                if status.camera_active { GREEN } else { MUTED },
            ),
        ];
        ui.columns(4, |cols| {
            for (col, (ic, label, value, color)) in cols.iter_mut().zip(tiles) {
                let tile = glass_frame(18).inner_margin(16).show(col, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
                        icon_disc(ui.painter(), rect.center(), 20.0, color, ic);
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 1.0;
                            ui.label(small(label));
                            ui.label(RichText::new(value).family(semibold()).size(16.0));
                        });
                    });
                });
                sheen(col.painter(), tile.response.rect, 18.0);
            }
        });
        ui.add_space(14.0);

        // Caméra
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(icon(I_VIDEO, 16.0).color(MUTED));
                ui.label(h2("Aperçu caméra"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    toggle(ui, &mut self.show_video);
                    ui.label(small("Afficher"));
                });
            });
            ui.add_space(8.0);
            let placeholder = if !connected {
                Some("Service arrêté")
            } else if !self.show_video {
                Some("Aperçu masqué : la surveillance continue en arrière-plan")
            } else if status.suspended {
                Some("Surveillance en pause")
            } else if status.locked {
                Some("Session verrouillée")
            } else if !status.camera_active {
                Some("Caméra inactive")
            } else if self.tex.is_none() {
                Some("En attente de la prochaine vérification…")
            } else {
                None
            };
            video(ui, self.tex.as_ref(), &self.faces, self.frame_size, placeholder);
        });
        ui.add_space(14.0);

        // Journal
        card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(icon(I_INFO, 16.0).color(MUTED));
                ui.label(h2("Activité"));
            });
            ui.add_space(6.0);
            let logs = self.link.lock().unwrap().logs.clone();
            egui::Frame::new()
                .fill(SUNKEN)
                .corner_radius(14)
                .inner_margin(12)
                .show(ui, |ui| {
                    egui::ScrollArea::both()
                        .max_height(260.0)
                        .stick_to_bottom(true)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if logs.is_empty() {
                                ui.label(small("Aucune activité pour l'instant."));
                            }
                            for line in &logs {
                                let (time, msg) = line
                                    .split_once("] ")
                                    .map(|(t, m)| (format!("{t}]"), m))
                                    .unwrap_or((String::new(), line));
                                let color = if msg.contains("[erreur]") {
                                    RED
                                } else if msg.starts_with('▶') {
                                    VIOLET
                                } else if msg.contains("décompte") || msg.contains("détect") {
                                    ORANGE
                                } else {
                                    Color32::from_rgb(200, 203, 212)
                                };
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(time).monospace().color(FAINT));
                                    ui.label(RichText::new(msg).monospace().color(color));
                                });
                            }
                        });
                });
        });
    }

    // ------------------------------------------------------------ workflows

    fn page_workflows(&mut self, ui: &mut egui::Ui) {
        let mut open_gallery = false;
        page_header(
            ui,
            "Workflows",
            "Quand quelque chose se passe devant ta caméra, FaceLock enchaîne tes actions.",
            |ui| {
                if button(ui, Some(I_ADD), "Nouveau workflow", Kind::Primary, true).clicked() {
                    open_gallery = true;
                }
            },
        );
        if open_gallery {
            self.gallery = true;
        }
        if self.cfg.workflows.is_empty() {
            card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(16.0);
                    ui.label(icon(I_FLOW, 34.0).color(VIOLET));
                    ui.label(h2("Aucun workflow"));
                    ui.label(muted(
                        "Pars d'un modèle : verrouillage auto, anti-regard indiscret, panic button…",
                    ));
                    ui.add_space(8.0);
                    if button(ui, Some(I_ADD), "Choisir un modèle", Kind::Primary, true).clicked() {
                        self.gallery = true;
                    }
                    ui.add_space(16.0);
                });
            });
            return;
        }
        let mut ops = Vec::new();
        let recording = match self.recording.as_ref().map(|r| r.0) {
            Some(Rec::Workflow(id)) => Some(id),
            Some(_) => Some(0),
            None => None,
        };
        let recording_action = match self.recording.as_ref().map(|r| r.0) {
            Some(Rec::Action(id, i)) => Some((id, i)),
            _ => None,
        };
        let mut workflows = std::mem::take(&mut self.cfg.workflows);
        for (i, wf) in workflows.iter_mut().enumerate() {
            let mut cx = flow::Ctx {
                sel: &mut self.sel,
                ops: &mut ops,
                recording,
                recording_action,
            };
            flow::workflow_card(ui, i, wf, &mut cx);
            ui.add_space(12.0);
        }
        self.cfg.workflows = workflows;
        ui.horizontal(|ui| {
            ui.label(icon(I_INFO, 14.0).color(FAINT));
            ui.label(small(
                "Clique sur un bloc pour le modifier. Les changements sont enregistrés et appliqués automatiquement.",
            ));
        });

        let ctx = ui.ctx().clone();
        for op in ops {
            match op {
                Op::Delete(i) => self.confirm = Some(Confirm::Workflow(i)),
                Op::Duplicate(i) => {
                    if let Some(w) = self.cfg.workflows.get(i).cloned() {
                        let copy = Workflow {
                            id: crate::config::new_id(),
                            name: format!("{} (copie)", w.name),
                            ..w
                        };
                        self.cfg.workflows.insert(i + 1, copy);
                    }
                }
                Op::Test(id) => self.pending_test = Some(id),
                Op::Browse(id, i) => {
                    self.picker = Some(Picker {
                        target: (id, i),
                        search: String::new(),
                        rx: None,
                        items: Vec::new(),
                        focus: true,
                    })
                }
                Op::Record(id) => self.start_recording(&ctx, Rec::Workflow(id)),
                Op::RecordAction(id, i) => self.start_recording(&ctx, Rec::Action(id, i)),
            }
        }
    }

    fn gallery_modal(&mut self, ctx: &egui::Context) {
        if !self.gallery {
            return;
        }
        let mut chosen = None;
        let mut close = false;
        // Largeur et hauteur adaptées à la fenêtre : rien ne déborde.
        let screen = ctx.content_rect();
        let width = (screen.width() - 120.0).clamp(360.0, 780.0);
        let cols = if width >= 600.0 { 2 } else { 1 };
        let tile_w = (width - 12.0 * (cols as f32 - 1.0)) / cols as f32;
        let list_h = (screen.height() - 260.0).max(200.0);
        let resp = egui::Modal::new(egui::Id::unique("gallery"))
            .frame(modal_frame())
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(h1("Nouveau workflow"));
                        ui.label(muted("Choisis un modèle, tu pourras tout personnaliser ensuite."));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                        if icon_button(ui, I_CLOSE, "Fermer", MUTED).clicked() {
                            close = true;
                        }
                    });
                });
                ui.add_space(12.0);
                egui::ScrollArea::vertical()
                    .max_height(list_h)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        // Rangées de `cols` tuiles ; chaque tuile empile son contenu verticalement
                        // et toutes les tuiles d'une rangée prennent la hauteur de la plus haute.
                        for (row, presets) in flow::PRESETS.chunks(cols).enumerate() {
                            let row_h = ui
                                .ctx()
                                .data(|d| d.get_temp::<f32>(egui::Id::unique(("preset-row", row))));
                            let mut tallest: f32 = 0.0;
                            ui.horizontal_top(|ui| {
                                ui.spacing_mut().item_spacing.x = 12.0;
                                for (j, preset) in presets.iter().enumerate() {
                                    let wf = (preset.build)();
                                    let meta = flow::trigger_meta(&wf.trigger);
                                    let id = egui::Id::unique(("preset", row * cols + j));
                                    let hovered = ui.ctx().read_response(id).is_some_and(|r| r.hovered());
                                    let tile = egui::Frame::new()
                                        .fill(if hovered { SURFACE3 } else { SURFACE2 })
                                        .stroke(Stroke::new(
                                            if hovered { 1.5 } else { 1.0 },
                                            if hovered { meta.color } else { BORDER },
                                        ))
                                        .corner_radius(16)
                                        .inner_margin(16)
                                        .show(ui, |ui| {
                                            ui.set_width(tile_w - 32.0);
                                            ui.set_min_height(row_h.unwrap_or(0.0));
                                            ui.vertical(|ui| {
                                                ui.horizontal(|ui| {
                                                    let (r, _) =
                                                        ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                                                    icon_disc(ui.painter(), r.center(), 17.0, meta.color, meta.icon);
                                                    ui.label(RichText::new(preset.title).family(semibold()).size(15.0));
                                                });
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(preset.desc).size(13.0).color(MUTED),
                                                    )
                                                    .wrap(),
                                                );
                                                ui.add_space(4.0);
                                                flow::mini_flow(ui, &wf);
                                            })
                                            .response
                                            .rect
                                            .height()
                                        });
                                    tallest = tallest.max(tile.inner);
                                    let resp = ui.interact(tile.response.rect, id, Sense::click());
                                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                        chosen = Some(wf.clone());
                                    }
                                }
                            });
                            if row_h != Some(tallest) {
                                ui.ctx()
                                    .data_mut(|d| d.insert_temp(egui::Id::unique(("preset-row", row)), tallest));
                                ui.ctx().request_repaint();
                            }
                            ui.add_space(12.0);
                        }
                    });
            });
        if resp.should_close() || close {
            self.gallery = false;
        }
        if let Some(wf) = chosen {
            let id = wf.id;
            let hotkey = matches!(wf.trigger, Trigger::Hotkey { .. });
            self.cfg.workflows.push(wf);
            self.sel = Some((id, Sel::Trigger));
            self.gallery = false;
            self.page = Page::Workflows;
            if hotkey {
                self.toast("Définis le raccourci dans le bloc « QUAND »", true);
            }
        }
    }

    // --------------------------------------------------------------- visages

    fn page_faces(&mut self, ui: &mut egui::Ui, status: &Status) {
        if self.enroll.is_some() {
            self.enroll_ui(ui, status);
            return;
        }
        let mut add = false;
        page_header(
            ui,
            "Visages",
            "Seuls ces visages comptent comme « toi ». Aucune photo n'est stockée, uniquement des empreintes.",
            |ui| {
                if button(ui, Some(I_ADD), "Ajouter un visage", Kind::Primary, true).clicked() {
                    add = true;
                }
            },
        );
        if add {
            self.enroll = Some(EnrollUi {
                name: String::new(),
                error: None,
                saving: false,
            });
            self.tex = None;
            self.faces.clear();
            self.send(Cmd::EnrollStart);
            return;
        }
        if self.profiles.is_empty() {
            card(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(18.0);
                    ui.label(icon(I_FACE, 36.0).color(ACCENT));
                    ui.label(h2("Aucun visage enrôlé"));
                    ui.label(muted("12 captures sous différents angles, environ une minute."));
                    ui.add_space(18.0);
                });
            });
            return;
        }
        let w = ((ui.available_width() - 24.0) / 3.0).max(200.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = Vec2::new(12.0, 12.0);
            for name in self.profiles.clone() {
                let (rect, _) = ui.allocate_exact_size(Vec2::new(w, 92.0), Sense::hover());
                let p = ui.painter();
                p.rect(rect, 18.0, SURFACE, Stroke::new(1.0, BORDER), StrokeKind::Inside);
                sheen(p, rect, 18.0);
                let hue = name
                    .bytes()
                    .fold(0u32, |a, b| a.wrapping_mul(31).wrapping_add(b as u32))
                    % 360;
                let color: Color32 = egui::ecolor::Hsva::new(hue as f32 / 360.0, 0.55, 0.9, 1.0).into();
                let c = egui::pos2(rect.left() + 46.0, rect.center().y);
                p.circle_filled(c, 26.0, color.gamma_multiply(0.25));
                let initials: String = name
                    .split_whitespace()
                    .filter_map(|s| s.chars().next())
                    .take(2)
                    .collect::<String>()
                    .to_uppercase();
                p.text(c, Align2::CENTER_CENTER, initials, FontId::new(17.0, semibold()), color);
                p.text(
                    egui::pos2(rect.left() + 86.0, rect.center().y - 10.0),
                    Align2::LEFT_CENTER,
                    &name,
                    FontId::new(15.5, semibold()),
                    TEXT,
                );
                p.text(
                    egui::pos2(rect.left() + 86.0, rect.center().y + 12.0),
                    Align2::LEFT_CENTER,
                    "Profil autorisé",
                    FontId::proportional(12.5),
                    MUTED,
                );
                let br =
                    egui::Rect::from_center_size(egui::pos2(rect.right() - 30.0, rect.center().y), Vec2::splat(32.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br));
                if icon_button(&mut child, I_DELETE, "Supprimer", RED).clicked() {
                    self.confirm = Some(Confirm::Profile(name.clone()));
                }
            }
        });
    }

    fn enroll_ui(&mut self, ui: &mut egui::Ui, status: &Status) {
        let info = status.enroll.clone().unwrap_or_default();
        let (mut capture, mut save, mut cancel) = (false, false, false);
        page_header(
            ui,
            "Ajouter un visage",
            "Regarde la caméra et varie légèrement l'angle entre chaque capture.",
            |ui| {
                if button(ui, Some(I_CLOSE), "Annuler", Kind::Ghost, true).clicked() {
                    cancel = true;
                }
            },
        );
        card(ui, |ui| {
            let en = self.enroll.as_mut().unwrap();
            ui.horizontal(|ui| {
                ui.label(muted("Nom du profil"));
                ui.add(
                    egui::TextEdit::singleline(&mut en.name)
                        .hint_text("ex. Alexandre")
                        .desired_width(260.0),
                );
            });
            ui.add_space(10.0);
            let placeholder = if self.tex.is_none() {
                Some("Ouverture de la caméra…")
            } else {
                None
            };
            video(ui, self.tex.as_ref(), &self.faces, self.frame_size, placeholder);
            ui.add_space(12.0);
            // Progression : une pastille par capture
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for i in 0..TARGET_SAMPLES {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(26.0, 8.0), Sense::hover());
                    let c = if i < info.count {
                        if info.count >= MIN_SAMPLES {
                            GREEN
                        } else {
                            ACCENT
                        }
                    } else {
                        SURFACE3
                    };
                    ui.painter().rect_filled(r, 4.0, c);
                }
                ui.add_space(8.0);
                ui.label(RichText::new(format!("{} / {TARGET_SAMPLES}", info.count)).family(semibold()));
                ui.add_space(8.0);
                if info.several {
                    ui.label(RichText::new("Un seul visage à la fois").color(RED));
                } else if !info.face_ok && self.tex.is_some() {
                    ui.label(muted("Aucun visage détecté"));
                } else if info.face_ok {
                    ui.label(RichText::new("Visage détecté").color(GREEN));
                }
            });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if button(ui, Some(I_VIDEO), "Capturer  (Espace)", Kind::Primary, info.face_ok).clicked() {
                    capture = true;
                }
                let saving = self.enroll.as_ref().is_some_and(|e| e.saving);
                if button(
                    ui,
                    Some(I_CHECK),
                    "Enregistrer le profil",
                    Kind::Tint(GREEN),
                    info.count >= MIN_SAMPLES && !saving,
                )
                .clicked()
                {
                    save = true;
                }
            });
            if let Some(err) = &self.enroll.as_ref().unwrap().error {
                ui.label(RichText::new(err).color(RED));
            }
            ui.add_space(6.0);
            ui.label(small(
                "Conseils : face, léger profil gauche et droite, avec et sans lunettes. 5 captures minimum.",
            ));
        });
        let typing = ui.ctx().memory(|m| m.focused().is_some());
        if !typing && ui.input(|i| i.key_pressed(egui::Key::Space)) {
            capture = true;
        }
        if capture && info.face_ok {
            self.send(Cmd::EnrollCapture);
        }
        if save {
            let en = self.enroll.as_mut().unwrap();
            en.error = None;
            let name = en.name.trim().to_string();
            if crate::engine::valid_profile_name(&name) {
                en.saving = true;
                let _ = self.tx.send((Cmd::EnrollSave { name }, true));
            } else {
                en.error = Some("Nom invalide : lettres, chiffres, espaces, tirets ; 32 caractères max.".into());
            }
        }
        if cancel {
            self.send(Cmd::EnrollCancel);
            self.enroll = None;
            self.tex = None;
        }
    }

    // --------------------------------------------------------------- réglages

    fn page_settings(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        page_header(
            ui,
            "Réglages",
            "Caméra, détection, raccourcis système et démarrage.",
            |_| {},
        );

        section(ui, "Caméra");
        let current = if self.cfg.camera_name.is_empty() {
            self.cameras
                .get(self.cfg.camera_index)
                .cloned()
                .unwrap_or_else(|| format!("Caméra {}", self.cfg.camera_index))
        } else {
            self.cfg.camera_name.clone()
        };
        let desc = if self.cam_rx.is_some() {
            "Détection des caméras…".to_string()
        } else {
            format!("{} caméra(s) détectée(s)", self.cameras.len())
        };
        let mut refresh = false;
        setting_row(ui, I_VIDEO, "Webcam", &desc, |ui| {
            if icon_button(ui, I_REFRESH, "Actualiser", MUTED).clicked() {
                refresh = true;
            }
            egui::ComboBox::from_id_salt("cam")
                .width(280.0)
                .selected_text(current)
                .show_ui(ui, |ui| {
                    for (i, name) in self.cameras.clone().into_iter().enumerate() {
                        if ui.selectable_label(self.cfg.camera_index == i, &name).clicked() {
                            self.cfg.camera_index = i;
                            self.cfg.camera_name = name;
                        }
                    }
                });
        });
        if refresh {
            self.refresh_cameras(&ctx);
        }

        section(ui, "Détection");
        setting_row(
            ui,
            I_CLOCK,
            "Intervalle de vérification",
            "Plus court = réaction plus rapide. ~10 ms de CPU par vérification.",
            |ui| {
                ui.add(
                    egui::Slider::new(&mut self.cfg.check_interval, 0.2..=10.0)
                        .logarithmic(true)
                        .suffix(" s")
                        .max_decimals(1),
                );
            },
        );
        setting_row(
            ui,
            I_EYE,
            "Seuil de reconnaissance",
            "0,363 recommandé. Plus haut = plus strict, moins de faux positifs.",
            |ui| {
                ui.add(
                    egui::Slider::new(&mut self.cfg.match_threshold, 0.2..=0.7)
                        .min_decimals(3)
                        .max_decimals(3),
                );
            },
        );

        section(ui, "Raccourcis système");
        for (target, title, desc) in [
            (
                Rec::Toggle,
                "Pause / reprise",
                "Met la surveillance en pause ou la relance",
            ),
            (
                Rec::Reset,
                "Réinitialiser",
                "Annule les décomptes et relance la vérification",
            ),
        ] {
            let combo = match target {
                Rec::Toggle => self.cfg.hotkeys.toggle.clone(),
                _ => self.cfg.hotkeys.reset.clone(),
            };
            let recording = self.recording.as_ref().is_some_and(|r| r.0 == target);
            let mut start = false;
            let mut clear = false;
            let free = self.recording.is_none();
            setting_row(ui, I_KEYBOARD, title, desc, |ui| {
                if !combo.is_empty() && icon_button(ui, I_CLOSE, "Effacer", MUTED).clicked() {
                    clear = true;
                }
                if button(
                    ui,
                    None,
                    if combo.is_empty() { "Définir" } else { "Modifier" },
                    Kind::Ghost,
                    free,
                )
                .clicked()
                {
                    start = true;
                }
                if recording {
                    ui.label(RichText::new("Appuie sur ta combinaison…").color(ORANGE));
                } else if combo.is_empty() {
                    ui.label(muted("non défini"));
                } else {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| keycaps(ui, &combo));
                }
            });
            if start {
                self.start_recording(&ctx, target);
            }
            if clear {
                self.set_combo(target, String::new());
            }
        }
        setting_row(
            ui,
            I_BELL,
            "Notifier ces raccourcis",
            "Affiche une notification système à chaque pause / réinitialisation",
            |ui| {
                let mut on = self.cfg.notifications.toggle && self.cfg.notifications.reset;
                if toggle(ui, &mut on).changed() {
                    self.cfg.notifications.toggle = on;
                    self.cfg.notifications.reset = on;
                }
            },
        );

        section(ui, "Système");
        let mut auto = self.autostart;
        let mut auto_changed = false;
        setting_row(
            ui,
            I_POWER,
            "Lancer à l'ouverture de session",
            "FaceLock démarre discrètement avec ta session",
            |ui| {
                auto_changed = toggle(ui, &mut auto).changed();
            },
        );
        if auto_changed {
            if winutil::set_autostart(auto) {
                self.autostart = auto;
            } else {
                self.toast("Impossible de modifier le démarrage automatique", false);
            }
        }
        let dir = crate::config::data_dir();
        setting_row(
            ui,
            I_FOLDER,
            "Données",
            &format!("{} — config et empreintes, rien ne sort de ta machine", dir.display()),
            |ui| {
                if button(ui, None, "Ouvrir le dossier", Kind::Ghost, true).clicked() {
                    winutil::open_folder(&dir);
                }
            },
        );
    }

    // ---------------------------------------------------------------- fenêtres

    fn picker_modal(&mut self, ctx: &egui::Context) {
        let Some(p) = self.picker.as_mut() else {
            return;
        };
        if p.rx.is_none() && p.items.is_empty() {
            let (tx, rx) = mpsc::channel();
            let c = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(winutil::list_processes());
                c.request_repaint();
            });
            p.rx = Some(rx);
        }
        if let Some(rx) = &p.rx {
            if let Ok(list) = rx.try_recv() {
                p.items = list
                    .into_iter()
                    .map(|e| {
                        let tex = e.icon.map(|rgba| {
                            let img = egui::ColorImage::from_rgba_unmultiplied([winutil::ICON, winutil::ICON], &rgba);
                            ctx.load_texture(format!("ico-{}", e.name), img, egui::TextureOptions::LINEAR)
                        });
                        (e.name, tex)
                    })
                    .collect();
                p.rx = None;
            }
        }
        let mut chosen = None;
        let resp = egui::Modal::new(egui::Id::unique("picker"))
            .frame(modal_frame())
            .show(ctx, |ui| {
                ui.set_width(440.0);
                ui.label(h2("Choisir une application"));
                ui.add_space(6.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut p.search)
                        .hint_text("Rechercher…")
                        .desired_width(f32::INFINITY),
                );
                if p.focus {
                    edit.request_focus();
                    p.focus = false;
                }
                ui.add_space(6.0);
                let needle = p.search.to_lowercase();
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if p.rx.is_some() {
                            ui.label(small("Chargement…"));
                        }
                        for (name, tex) in p
                            .items
                            .iter()
                            .filter(|(n, _)| needle.is_empty() || n.to_lowercase().contains(&needle))
                        {
                            let (rect, resp) =
                                ui.allocate_exact_size(Vec2::new(ui.available_width(), 36.0), Sense::click());
                            if resp.hovered() {
                                ui.painter().rect_filled(rect, 8.0, SURFACE3);
                            }
                            let ir = egui::Rect::from_center_size(
                                egui::pos2(rect.left() + 20.0, rect.center().y),
                                Vec2::splat(20.0),
                            );
                            if let Some(t) = tex {
                                ui.painter().image(
                                    t.id(),
                                    ir,
                                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                                    Color32::WHITE,
                                );
                            } else {
                                ui.painter().rect_filled(ir, 5.0, SURFACE3);
                            }
                            ui.painter().text(
                                egui::pos2(rect.left() + 42.0, rect.center().y),
                                Align2::LEFT_CENTER,
                                name,
                                FontId::proportional(14.0),
                                TEXT,
                            );
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                chosen = Some(name.clone());
                            }
                        }
                    });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if button(ui, Some(I_REFRESH), "Actualiser", Kind::Ghost, true).clicked() {
                        p.items.clear();
                    }
                    if button(ui, None, "Fermer", Kind::Ghost, true).clicked() {
                        chosen = Some(String::new());
                    }
                });
            });
        if resp.should_close() && chosen.is_none() {
            chosen = Some(String::new());
        }
        if let Some(name) = chosen {
            let (id, i) = p.target;
            self.picker = None;
            if !name.is_empty() {
                if let Some(a) = self
                    .cfg
                    .workflows
                    .iter_mut()
                    .find(|w| w.id == id)
                    .and_then(|w| w.actions.get_mut(i))
                {
                    a.params.process = name;
                }
            }
        }
    }

    fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(what) = &self.confirm else { return };
        let (title, text) = match what {
            Confirm::Profile(n) => (
                "Supprimer le profil".to_string(),
                format!("Le profil « {n} » sera définitivement supprimé."),
            ),
            Confirm::Workflow(i) => (
                "Supprimer le workflow".to_string(),
                format!(
                    "« {} » sera supprimé.",
                    self.cfg.workflows.get(*i).map(|w| w.name.as_str()).unwrap_or("?")
                ),
            ),
        };
        let (mut yes, mut no) = (false, false);
        let resp = egui::Modal::new(egui::Id::unique("confirm"))
            .frame(modal_frame())
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(h2(&title));
                ui.add_space(4.0);
                ui.label(muted(&text));
                ui.add_space(14.0);
                ui.horizontal(|ui| {
                    if button(ui, Some(I_DELETE), "Supprimer", Kind::Danger, true).clicked() {
                        yes = true;
                    }
                    if button(ui, None, "Annuler", Kind::Ghost, true).clicked() {
                        no = true;
                    }
                });
            });
        if yes {
            match self.confirm.take() {
                Some(Confirm::Profile(name)) => {
                    match std::fs::remove_file(crate::config::faces_dir().join(format!("{name}.face"))) {
                        Ok(()) => {
                            self.send(Cmd::ReloadProfiles);
                            self.toast(format!("Profil « {name} » supprimé"), true);
                        }
                        Err(e) => self.toast(format!("Suppression impossible : {e}"), false),
                    }
                    self.refresh_profiles();
                }
                Some(Confirm::Workflow(i)) if i < self.cfg.workflows.len() => {
                    self.cfg.workflows.remove(i);
                    self.sel = None;
                }
                Some(Confirm::Workflow(_)) => {}
                None => {}
            }
        } else if no || resp.should_close() {
            self.confirm = None;
        }
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((msg, at, ok)) = &self.toast else {
            return;
        };
        let life = Duration::from_millis(2600);
        if at.elapsed() >= life {
            self.toast = None;
            return;
        }
        ctx.request_repaint_after(life - at.elapsed());
        let (msg, ok) = (msg.clone(), *ok);
        egui::Area::new(egui::Id::unique("toast"))
            .anchor(Align2::RIGHT_BOTTOM, Vec2::new(-24.0, -24.0))
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(22, 24, 34, 240))
                    .stroke(Stroke::new(
                        1.0,
                        if ok {
                            GREEN.gamma_multiply(0.5)
                        } else {
                            RED.gamma_multiply(0.6)
                        },
                    ))
                    .corner_radius(12)
                    .inner_margin(egui::Margin::symmetric(16, 12))
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 6],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(120),
                    })
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(icon(if ok { I_CHECK } else { I_WARN }, 15.0).color(if ok { GREEN } else { RED }));
                            ui.label(RichText::new(msg).color(TEXT));
                        });
                    });
            });
    }
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(
        RichText::new(title.to_uppercase())
            .family(semibold())
            .size(11.5)
            .color(FAINT),
    );
    ui.add_space(2.0);
}

fn modal_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(Color32::from_rgba_premultiplied(16, 18, 28, 235))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(20)
        .inner_margin(24)
        .shadow(egui::epaint::Shadow {
            offset: [0, 12],
            blur: 40,
            spread: 0,
            color: Color32::from_black_alpha(150),
        })
}

/// Aperçu caméra + cadres des visages (dessinés par egui, pas dans l'image).
fn video(
    ui: &mut egui::Ui,
    tex: Option<&egui::TextureHandle>,
    faces: &[FaceBox],
    size: (usize, usize),
    placeholder: Option<&str>,
) {
    let w = ui.available_width().min(720.0);
    let h = if placeholder.is_some() {
        150.0
    } else {
        w * size.1.max(1) as f32 / size.0.max(1) as f32
    };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 16.0, SUNKEN);
    match (placeholder, tex) {
        (None, Some(tex)) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.add(egui::epaint::RectShape::filled(rect, 14.0, Color32::WHITE).with_texture(tex.id(), uv));
            let sx = w / size.0 as f32;
            let sy = h / size.1 as f32;
            for f in faces {
                let r = egui::Rect::from_min_size(
                    rect.min + Vec2::new(f.bbox[0] * sx, f.bbox[1] * sy),
                    Vec2::new(f.bbox[2] * sx, f.bbox[3] * sy),
                );
                let color = if f.name.is_some() { GREEN } else { RED };
                painter.rect_stroke(r, 10.0, Stroke::new(2.5, color), StrokeKind::Outside);
                let text = match &f.name {
                    Some(n) if n.is_empty() => continue,
                    Some(n) => format!("{n}  {:.0} %", f.score * 100.0),
                    None => format!("Inconnu  {:.0} %", f.score.max(0.0) * 100.0),
                };
                let galley = painter.layout_no_wrap(text, FontId::new(12.5, semibold()), Color32::WHITE);
                let pos = egui::pos2(r.min.x, (r.min.y - galley.size().y - 10.0).max(rect.min.y + 4.0));
                let bg = egui::Rect::from_min_size(pos, galley.size() + Vec2::new(14.0, 6.0));
                painter.rect_filled(bg, 8.0, color);
                painter.galley(pos + Vec2::new(7.0, 3.0), galley, Color32::WHITE);
            }
        }
        _ => {
            painter.text(
                rect.center() - Vec2::new(0.0, 12.0),
                Align2::CENTER_CENTER,
                I_VIDEO,
                FontId::new(22.0, icons()),
                FAINT,
            );
            painter.text(
                rect.center() + Vec2::new(0.0, 16.0),
                Align2::CENTER_CENTER,
                placeholder.unwrap_or("En attente de la caméra…"),
                FontId::proportional(14.0),
                MUTED,
            );
        }
    }
}
