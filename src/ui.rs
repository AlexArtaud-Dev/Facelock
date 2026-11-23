//! Interface (processus séparé, lancé à la demande depuis l'icône).
//! Rendu immédiat egui : aucune image recalculée tant que rien ne change.
//! Fermer la fenêtre termine le processus et libère toute sa mémoire.

use crate::actions::{label, ACTIONS};
use crate::config::{ActionConf, Config};
use crate::hotkey;
use crate::ipc::{self, Cmd, FaceBox, FrameMeta, Reply, Req, Status};
use crate::winutil;
use eframe::egui::{self, Align, Align2, Color32, CornerRadius, FontId, Layout, RichText, Sense, Stroke, Vec2};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const BG: Color32 = Color32::from_rgb(22, 23, 27);
const CARD: Color32 = Color32::from_rgb(32, 34, 40);
const VIDEO_BG: Color32 = Color32::from_rgb(14, 14, 18);
const MUTED: Color32 = Color32::from_rgb(140, 142, 150);
const GREEN: Color32 = Color32::from_rgb(53, 194, 94);
const RED: Color32 = Color32::from_rgb(230, 70, 70);
const ORANGE: Color32 = Color32::from_rgb(224, 160, 48);
const ACCENT: Color32 = Color32::from_rgb(30, 120, 220);
const TARGET_SAMPLES: usize = 12;
const MIN_SAMPLES: usize = 5;

pub fn run(page: &str) {
    let icon = egui::IconData { rgba: crate::icon::rgba(64, crate::icon::ACTIVE), width: 64, height: 64 };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FaceLock")
            .with_inner_size([780.0, 780.0])
            .with_min_inner_size([620.0, 520.0])
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

type Outbox = Sender<(Cmd, bool)>;

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
                        Ok((r, _)) if want_reply => {
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
            if ipc::write_msg(&mut pipe, &Req::Poll { log_since, want_frame, frame_seq }, &[]).is_err() {
                break;
            }
            let Ok((reply, bin)) = ipc::read_msg::<_, Reply>(&mut pipe) else { break };
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
            if let Some(nav) = st.nav.clone() {
                l.status.nav = Some(nav);
            }
            let nav = l.status.nav.take();
            l.status = Status { logs: Vec::new(), frame: None, nav, ..st };
            drop(l);
            if changed {
                ctx.request_repaint();
            }
        }
        link.lock().unwrap().connected = false;
        ctx.request_repaint();
    }
}

// ------------------------------------------------------------------ app

#[derive(PartialEq, Clone, Copy)]
enum Page {
    Live,
    General,
    Faces,
    Triggers,
    Shortcuts,
}

#[derive(Clone, Copy, PartialEq)]
enum ListId {
    Absence,
    Stranger,
    Panic,
}

#[derive(Clone, Copy, PartialEq)]
enum HkField {
    Toggle,
    Reset,
    Panic,
}

struct Picker {
    target: (ListId, usize),
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

struct App {
    page: Page,
    link: Arc<Mutex<Link>>,
    tx: Outbox,
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
    confirm_delete: Option<String>,
    picker: Option<Picker>,
    recording: Option<(HkField, Receiver<String>)>,
    autostart: bool,
}

fn setup_style(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |key: &str, file: &str, fam: egui::FontFamily, first: bool| {
        if let Ok(bytes) = std::fs::read(format!("C:\\Windows\\Fonts\\{file}")) {
            fonts.font_data.insert(key.into(), Arc::new(egui::FontData::from_owned(bytes)));
            let list = fonts.families.entry(fam).or_default();
            if first {
                list.insert(0, key.into());
            } else {
                list.push(key.into());
            }
        }
    };
    add("segoe", "segoeui.ttf", egui::FontFamily::Proportional, true);
    add("segoe-sym", "seguisym.ttf", egui::FontFamily::Proportional, false);
    add("consolas", "consola.ttf", egui::FontFamily::Monospace, true);
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(8.0, 7.0);
        s.spacing.button_padding = Vec2::new(12.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.spacing.slider_width = 240.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(14.5));
        s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(19.0));
        s.text_styles.insert(egui::TextStyle::Monospace, FontId::monospace(12.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.5));
        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = CARD;
        v.extreme_bg_color = Color32::from_rgb(18, 18, 22);
        v.selection.bg_fill = ACCENT;
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
            &mut v.widgets.noninteractive,
        ] {
            w.corner_radius = CornerRadius::same(6);
        }
        v.widgets.inactive.weak_bg_fill = Color32::from_rgb(48, 50, 58);
        v.widgets.inactive.bg_fill = Color32::from_rgb(48, 50, 58);
        v.widgets.hovered.weak_bg_fill = Color32::from_rgb(60, 63, 73);
        v.widgets.hovered.bg_fill = Color32::from_rgb(60, 63, 73);
    });
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>, page: &str) -> Self {
        setup_style(&cc.egui_ctx);
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
            confirm_delete: None,
            picker: None,
            recording: None,
            autostart: winutil::autostart_enabled(),
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

    /// Synchronise l'état reçu du daemon (navigation, image, réponses).
    fn pump(&mut self, ctx: &egui::Context) {
        let wants_video = (self.page == Page::Live && self.show_video) || self.enroll.is_some();
        let mut l = self.link.lock().unwrap();
        l.want_frame = wants_video;
        l.fast = self.enroll.is_some();
        if let Some(nav) = l.status.nav.take() {
            if nav == "live" || self.page == Page::Live {
                self.page = page_from(&nav);
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        if let Some((meta, bin)) = l.frame.take() {
            if meta.seq != self.tex_seq && bin.len() == meta.w * meta.h * 4 {
                let mut rgba = bin;
                for px in rgba.chunks_exact_mut(4) {
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
        let replies: Vec<Reply> = l.replies.drain(..).collect();
        drop(l);
        for r in replies {
            if let Some(en) = self.enroll.as_mut() {
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
            }
        }
        if let Some(rx) = &self.cam_rx {
            if let Ok(list) = rx.try_recv() {
                self.cameras = list;
                self.cam_rx = None;
            }
        }
        if let Some((field, rx)) = &self.recording {
            if let Ok(combo) = rx.try_recv() {
                let field = *field;
                self.recording = None;
                self.send(Cmd::PauseHotkeys { paused: false });
                if !combo.is_empty() {
                    match hotkey::parse_combo(&combo) {
                        Ok(_) => *self.hk_mut(field) = combo,
                        Err(e) => self.toast(format!("Raccourci refusé : {e}"), false),
                    }
                }
            }
        }
    }

    fn hk_mut(&mut self, f: HkField) -> &mut String {
        match f {
            HkField::Toggle => &mut self.cfg.hotkeys.toggle,
            HkField::Reset => &mut self.cfg.hotkeys.reset,
            HkField::Panic => &mut self.cfg.panic.hotkey,
        }
    }

    fn autosave(&mut self, ctx: &egui::Context) {
        if self.cfg == self.saved {
            self.dirty_since = None;
            return;
        }
        let since = *self.dirty_since.get_or_insert_with(Instant::now);
        let wait = Duration::from_millis(350);
        if since.elapsed() < wait {
            ctx.request_repaint_after(wait - since.elapsed());
            return;
        }
        self.cfg.sanitize();
        match self.cfg.save() {
            Ok(()) => {
                self.saved = self.cfg.clone();
                self.send(Cmd::ReloadConfig);
                self.toast("✓ Paramètres enregistrés", true);
            }
            Err(e) => {
                self.saved = self.cfg.clone();
                self.toast(format!("Enregistrement impossible : {e}"), false);
            }
        }
        self.dirty_since = None;
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if self.enroll.is_some() {
            ipc::send_once(Cmd::EnrollCancel);
        }
        if self.recording.is_some() {
            ipc::send_once(Cmd::PauseHotkeys { paused: false });
        }
    }
}

// --------------------------------------------------------------- widgets

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(10)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

fn title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).strong().size(15.5));
}

fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(MUTED).small());
}

fn colored_button(ui: &mut egui::Ui, text: &str, fill: Color32, enabled: bool) -> egui::Response {
    ui.add_enabled(enabled, egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(fill))
}

fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 6.0, color);
}

/// Aperçu caméra + cadres des visages (dessinés par egui, pas dans l'image).
fn video(ui: &mut egui::Ui, tex: Option<&egui::TextureHandle>, faces: &[FaceBox], size: (usize, usize), placeholder: Option<&str>) {
    let w = ui.available_width().min(640.0);
    let h = if placeholder.is_some() { 120.0 } else { w * size.1.max(1) as f32 / size.0.max(1) as f32 };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 10.0, VIDEO_BG);
    match (placeholder, tex) {
        (None, Some(tex)) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            painter.image(tex.id(), rect, uv, Color32::WHITE);
            let sx = w / size.0 as f32;
            let sy = h / size.1 as f32;
            for f in faces {
                let r = egui::Rect::from_min_size(
                    rect.min + Vec2::new(f.bbox[0] * sx, f.bbox[1] * sy),
                    Vec2::new(f.bbox[2] * sx, f.bbox[3] * sy),
                );
                let color = if f.name.is_some() { GREEN } else { RED };
                painter.rect_stroke(r, 4.0, Stroke::new(2.0, color), egui::StrokeKind::Outside);
                let text = match &f.name {
                    Some(n) if n.is_empty() => continue,
                    Some(n) => format!("{n}  {:.2}", f.score),
                    None => format!("Inconnu  {:.2}", f.score),
                };
                let galley = painter.layout_no_wrap(text, FontId::proportional(13.0), Color32::WHITE);
                let pos = egui::pos2(r.min.x - 2.0, (r.min.y - galley.size().y - 6.0).max(rect.min.y));
                let bg = egui::Rect::from_min_size(pos, galley.size() + Vec2::new(10.0, 4.0));
                painter.rect_filled(bg, 4.0, color);
                painter.galley(pos + Vec2::new(5.0, 2.0), galley, Color32::WHITE);
            }
        }
        _ => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                placeholder.unwrap_or("En attente de la caméra…"),
                FontId::proportional(15.0),
                MUTED,
            );
        }
    }
}

// ------------------------------------------------------------------ pages

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.pump(&ctx);
        let (connected, status) = {
            let l = self.link.lock().unwrap();
            (l.connected, l.status.clone())
        };

        egui::Panel::top("nav")
            .frame(egui::Frame::new().fill(Color32::from_rgb(28, 29, 34)).inner_margin(egui::Margin::symmetric(14, 10)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    for (p, name) in [
                        (Page::Live, "Vue en direct"),
                        (Page::General, "Général"),
                        (Page::Faces, "Visages"),
                        (Page::Triggers, "Déclencheurs"),
                        (Page::Shortcuts, "Raccourcis"),
                    ] {
                        let sel = self.page == p;
                        let text = RichText::new(name).size(15.0);
                        if ui.add(egui::Button::selectable(sel, if sel { text.strong() } else { text })).clicked() {
                            self.page = p;
                        }
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let (color, text) = status_line(connected, &status);
                        ui.label(RichText::new(text).color(MUTED));
                        dot(ui, color);
                    });
                });
            });

        egui::Panel::bottom("toast")
            .frame(egui::Frame::new().fill(BG).inner_margin(egui::Margin::symmetric(14, 6)))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    hint(ui, "Données 100 % locales — %APPDATA%\\FaceLock");
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if let Some((msg, at, ok)) = &self.toast {
                            if at.elapsed() < Duration::from_millis(2500) {
                                ui.label(RichText::new(msg).color(if *ok { GREEN } else { RED }));
                                ctx.request_repaint_after(Duration::from_millis(2500) - at.elapsed());
                            }
                        }
                    });
                });
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BG).inner_margin(14))
            .show(ui, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| match self.page {
                    Page::Live => self.page_live(ui, connected, &status),
                    Page::General => self.page_general(ui),
                    Page::Faces => self.page_faces(ui, &status),
                    Page::Triggers => self.page_triggers(ui),
                    Page::Shortcuts => self.page_shortcuts(ui),
                });
            });

        self.picker_modal(&ctx);
        self.delete_modal(&ctx);
        self.autosave(&ctx);
    }
}

fn page_from(name: &str) -> Page {
    match name {
        "settings" | "general" => Page::General,
        "faces" => Page::Faces,
        "triggers" => Page::Triggers,
        "shortcuts" => Page::Shortcuts,
        _ => Page::Live,
    }
}

fn status_line(connected: bool, s: &Status) -> (Color32, String) {
    if !connected {
        (MUTED, "Service FaceLock non démarré".into())
    } else if s.enroll.is_some() {
        (ACCENT, "Enrôlement en cours".into())
    } else if s.suspended {
        (ORANGE, "Surveillance suspendue".into())
    } else if s.locked {
        (MUTED, "Session verrouillée — caméra coupée".into())
    } else if s.profiles == 0 {
        (MUTED, "Aucun visage enrôlé".into())
    } else if s.camera_active {
        (GREEN, "Surveillance active".into())
    } else {
        (MUTED, "Caméra inactive".into())
    }
}

impl App {
    fn page_live(&mut self, ui: &mut egui::Ui, connected: bool, status: &Status) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (color, text) = status_line(connected, status);
                dot(ui, color);
                ui.label(RichText::new(text).strong().size(16.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(connected, egui::Button::new("Réinitialiser")).on_hover_text(
                        "Annule les décomptes en cours, lève la suspension et relance la vérification",
                    ).clicked() {
                        self.send(Cmd::Reset);
                    }
                    let (txt, fill) = if status.suspended { ("Reprendre", GREEN.gamma_multiply(0.75)) } else { ("Suspendre", ORANGE.gamma_multiply(0.75)) };
                    if colored_button(ui, txt, fill, connected).clicked() {
                        self.send(Cmd::ToggleSuspend);
                    }
                });
            });
            if connected && status.profiles == 0 {
                hint(ui, "Aucun visage enrôlé : la surveillance démarrera après l'ajout d'un visage (onglet Visages).");
            } else if connected {
                hint(ui, &format!("{} profil(s) enrôlé(s)", status.profiles));
            }
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Caméra");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut self.show_video, "Afficher l'aperçu");
                });
            });
            let placeholder = if !connected {
                Some("Service non démarré")
            } else if !self.show_video {
                Some("Aperçu désactivé (la surveillance continue en arrière-plan)")
            } else if status.suspended {
                Some("Surveillance suspendue")
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
        ui.add_space(8.0);
        card(ui, |ui| {
            title(ui, "Journal");
            let logs = self.link.lock().unwrap().logs.clone();
            egui::Frame::new().fill(VIDEO_BG).corner_radius(6).inner_margin(8).show(ui, |ui| {
                egui::ScrollArea::both().max_height(240.0).stick_to_bottom(true).auto_shrink([false, true]).show(ui, |ui| {
                    for line in &logs {
                        let color = if line.contains("[erreur]") {
                            RED
                        } else if line.contains("PANIC") || line.contains("Action :") || line.contains("détecté") {
                            ORANGE
                        } else {
                            Color32::from_rgb(200, 200, 205)
                        };
                        ui.label(RichText::new(line).monospace().color(color));
                    }
                });
            });
        });
    }

    fn page_general(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        card(ui, |ui| {
            title(ui, "Webcam");
            ui.horizontal(|ui| {
                let current = if self.cfg.camera_name.is_empty() {
                    self.cameras
                        .get(self.cfg.camera_index)
                        .cloned()
                        .unwrap_or_else(|| format!("Caméra {}", self.cfg.camera_index))
                } else {
                    self.cfg.camera_name.clone()
                };
                egui::ComboBox::from_id_salt("cam").width(340.0).selected_text(current).show_ui(ui, |ui| {
                    for (i, name) in self.cameras.clone().into_iter().enumerate() {
                        let sel = self.cfg.camera_index == i;
                        if ui.selectable_label(sel, &name).clicked() {
                            self.cfg.camera_index = i;
                            self.cfg.camera_name = name;
                        }
                    }
                });
                if ui.button("Actualiser").clicked() {
                    self.refresh_cameras(&ctx);
                }
            });
            if self.cam_rx.is_some() {
                hint(ui, "Détection des caméras…");
            } else {
                hint(ui, &format!("{} caméra(s) détectée(s)", self.cameras.len()));
            }
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            title(ui, "Détection");
            ui.add(
                egui::Slider::new(&mut self.cfg.check_interval, 0.2..=10.0)
                    .logarithmic(true)
                    .suffix(" s")
                    .max_decimals(1)
                    .text("Intervalle entre deux vérifications"),
            );
            hint(ui, "Plus court = réaction plus rapide. Une vérification coûte ~10 ms de CPU (reconnaissance faciale uniquement si un visage bouge).");
            ui.add_space(4.0);
            ui.add(
                egui::Slider::new(&mut self.cfg.match_threshold, 0.2..=0.7)
                    .min_decimals(3)
                    .max_decimals(3)
                    .text("Seuil de reconnaissance"),
            );
            hint(ui, "0.363 = valeur recommandée par OpenCV. Plus haut = plus strict (moins de faux positifs).");
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            title(ui, "Démarrage");
            let mut on = self.autostart;
            if ui.checkbox(&mut on, "Lancer FaceLock à l'ouverture de session").changed() {
                if winutil::set_autostart(on) {
                    self.autostart = on;
                } else {
                    self.toast("Impossible de modifier le démarrage automatique", false);
                }
            }
            if ui.button("Ouvrir le dossier des données").clicked() {
                let _ = std::process::Command::new("explorer").arg(crate::config::data_dir()).spawn();
            }
        });
    }

    fn page_faces(&mut self, ui: &mut egui::Ui, status: &Status) {
        if self.enroll.is_some() {
            self.enroll_ui(ui, status);
            return;
        }
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Profils enrôlés");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if colored_button(ui, "+  Ajouter un visage", ACCENT, true).clicked() {
                        self.enroll = Some(EnrollUi { name: String::new(), error: None, saving: false });
                        self.tex = None;
                        self.faces.clear();
                        self.send(Cmd::EnrollStart);
                    }
                });
            });
            ui.add_space(4.0);
            if self.profiles.is_empty() {
                hint(ui, "Aucun profil — ajoute un visage pour activer la surveillance.");
            }
            for name in self.profiles.clone() {
                egui::Frame::new().fill(Color32::from_rgb(40, 42, 50)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&name).strong());
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if colored_button(ui, "Supprimer", RED.gamma_multiply(0.6), true).clicked() {
                                self.confirm_delete = Some(name.clone());
                            }
                        });
                    });
                });
            }
            ui.add_space(4.0);
            hint(ui, "Empreintes mathématiques uniquement (vecteurs 128-D) — aucune photo n'est stockée.");
        });
    }

    fn enroll_ui(&mut self, ui: &mut egui::Ui, status: &Status) {
        let info = status.enroll.clone().unwrap_or_default();
        let mut capture = false;
        let mut save = false;
        let mut cancel = false;
        card(ui, |ui| {
            title(ui, "Enrôler un visage");
            let en = self.enroll.as_mut().unwrap();
            ui.horizontal(|ui| {
                ui.label("Nom du profil");
                ui.add(egui::TextEdit::singleline(&mut en.name).hint_text("ex. Alexandre").desired_width(240.0));
            });
            let text_focused = ui.ctx().memory(|m| m.focused().is_some());
            if !text_focused && ui.input(|i| i.key_pressed(egui::Key::Space)) {
                capture = true;
            }
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            let placeholder = if self.tex.is_none() { Some("Ouverture de la caméra…") } else { None };
            video(ui, self.tex.as_ref(), &self.faces, self.frame_size, placeholder);
            ui.add_space(6.0);
            let frac = (info.count as f32 / TARGET_SAMPLES as f32).min(1.0);
            ui.add(egui::ProgressBar::new(frac).text(format!("Échantillons capturés : {}/{TARGET_SAMPLES}", info.count)));
            if info.several {
                ui.label(RichText::new("Un seul visage à la fois !").color(RED));
            } else if !info.face_ok && self.tex.is_some() {
                ui.label(RichText::new("Aucun visage détecté").color(MUTED));
            }
            ui.horizontal(|ui| {
                if colored_button(ui, "Capturer  (Espace)", ACCENT, info.face_ok).clicked() {
                    capture = true;
                }
                let en = self.enroll.as_ref().unwrap();
                if colored_button(ui, "Enregistrer le profil", GREEN.gamma_multiply(0.7), info.count >= MIN_SAMPLES && !en.saving)
                    .clicked()
                {
                    save = true;
                }
                if ui.button("Annuler").clicked() {
                    cancel = true;
                }
            });
            if let Some(err) = &self.enroll.as_ref().unwrap().error {
                ui.label(RichText::new(err).color(RED));
            }
            hint(ui, "Varie les poses : face, léger profil gauche/droite, avec/sans lunettes. 12 captures conseillées, 5 minimum.");
        });
        if capture && info.face_ok {
            self.send(Cmd::EnrollCapture);
        }
        if save {
            let en = self.enroll.as_mut().unwrap();
            en.saving = true;
            en.error = None;
            let name = en.name.trim().to_string();
            if !crate::engine::valid_profile_name(&name) {
                en.saving = false;
                en.error = Some("Nom invalide : lettres, chiffres, espaces, tirets ; 32 caractères max.".into());
            } else {
                let _ = self.tx.send((Cmd::EnrollSave { name }, true));
            }
        }
        if cancel {
            self.send(Cmd::EnrollCancel);
            self.enroll = None;
            self.tex = None;
        }
    }

    fn page_triggers(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Absence de visage autorisé");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut self.cfg.triggers.absence.enabled, "Activé");
                });
            });
            hint(ui, "Se déclenche quand aucun visage enrôlé n'est visible pendant le délai de grâce.");
            ui.add(egui::Slider::new(&mut self.cfg.triggers.absence.grace, 0.0..=300.0).suffix(" s").max_decimals(0).text("Délai de grâce"));
            ui.add_space(4.0);
            let mut list = std::mem::take(&mut self.cfg.triggers.absence.actions);
            self.actions_editor(ui, &mut list, ListId::Absence);
            self.cfg.triggers.absence.actions = list;
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Visage inconnu détecté");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut self.cfg.triggers.stranger.enabled, "Activé");
                });
            });
            hint(ui, "Se déclenche quand un visage non enrôlé est devant l'écran.");
            ui.checkbox(&mut self.cfg.triggers.stranger.only_when_absent, "Uniquement si aucun visage autorisé n'est présent");
            ui.add(egui::Slider::new(&mut self.cfg.triggers.stranger.grace, 0.0..=60.0).suffix(" s").max_decimals(0).text("Présence continue avant action"));
            ui.add(egui::Slider::new(&mut self.cfg.stranger_cooldown, 0.0..=300.0).suffix(" s").max_decimals(0).text("Anti-répétition"));
            ui.add_space(4.0);
            let mut list = std::mem::take(&mut self.cfg.triggers.stranger.actions);
            self.actions_editor(ui, &mut list, ListId::Stranger);
            self.cfg.triggers.stranger.actions = list;
        });
    }

    fn actions_editor(&mut self, ui: &mut egui::Ui, list: &mut Vec<ActionConf>, id: ListId) {
        ui.label(RichText::new("Actions (exécutées dans l'ordre)").color(MUTED));
        let mut remove = None;
        let mut swap = None;
        let n = list.len();
        for (i, a) in list.iter_mut().enumerate() {
            egui::Frame::new().fill(Color32::from_rgb(40, 42, 50)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{}.", i + 1)).color(MUTED));
                    egui::ComboBox::from_id_salt((id as u8, i, "action"))
                        .width(300.0)
                        .selected_text(label(&a.action))
                        .show_ui(ui, |ui| {
                            for (aid, lab, _, _) in ACTIONS {
                                ui.selectable_value(&mut a.action, aid.to_string(), *lab);
                            }
                        });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("✕").on_hover_text("Retirer").clicked() {
                            remove = Some(i);
                        }
                        if ui.add_enabled(i + 1 < n, egui::Button::new("▼")).clicked() {
                            swap = Some((i, i + 1));
                        }
                        if ui.add_enabled(i > 0, egui::Button::new("▲")).clicked() {
                            swap = Some((i - 1, i));
                        }
                    });
                });
                let (_, _, needs_proc, needs_desk) =
                    ACTIONS.iter().find(|x| x.0 == a.action).copied().unwrap_or(("", "", false, false));
                if needs_proc || needs_desk {
                    ui.horizontal(|ui| {
                        if needs_proc {
                            ui.label("Processus");
                            ui.add(egui::TextEdit::singleline(&mut a.params.process).hint_text("ex. chrome.exe").desired_width(180.0));
                            if ui.button("Parcourir…").clicked() {
                                self.picker = Some(Picker {
                                    target: (id, i),
                                    search: String::new(),
                                    rx: None,
                                    items: Vec::new(),
                                    focus: true,
                                });
                            }
                        }
                        if needs_desk {
                            let def = if a.action == "move_app" { 2 } else { 1 };
                            let mut d = a.params.desktop.unwrap_or(def);
                            ui.label("Bureau n°");
                            if ui.add(egui::DragValue::new(&mut d).range(1..=20)).changed() || a.params.desktop.is_none() {
                                a.params.desktop = Some(d);
                            }
                        }
                    });
                }
                if needs_proc {
                    ui.horizontal(|ui| {
                        ui.label("Titre de fenêtre contient");
                        ui.add(egui::TextEdit::singleline(&mut a.params.title).hint_text("optionnel, ex. YouTube").desired_width(220.0));
                    });
                }
            });
        }
        if let Some(i) = remove {
            list.remove(i);
        }
        if let Some((a, b)) = swap {
            list.swap(a, b);
        }
        if ui.button("+  Ajouter une action").clicked() {
            list.push(ActionConf::default());
        }
    }

    fn hotkey_row(&mut self, ui: &mut egui::Ui, text: &str, field: HkField) {
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(260.0, 26.0), Layout::left_to_right(Align::Center), |ui| {
                ui.set_min_width(260.0);
                ui.label(text);
            });
            let recording = self.recording.as_ref().is_some_and(|r| r.0 == field);
            let current = self.hk_mut(field).clone();
            let shown = if recording {
                "Appuie sur les touches…".to_string()
            } else if current.is_empty() {
                "Non défini — cliquer pour enregistrer".to_string()
            } else {
                hotkey::pretty(&current)
            };
            let btn = egui::Button::new(RichText::new(shown).color(if recording { ORANGE } else { Color32::WHITE }));
            if ui.add_enabled(self.recording.is_none(), btn.min_size(Vec2::new(240.0, 26.0))).clicked() {
                self.send(Cmd::PauseHotkeys { paused: true });
                let (tx, rx) = mpsc::channel();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let _ = tx.send(winutil::record_combo());
                    ctx.request_repaint();
                });
                self.recording = Some((field, rx));
            }
            if ui.add_enabled(!current.is_empty() && self.recording.is_none(), egui::Button::new("✕")).on_hover_text("Effacer").clicked() {
                self.hk_mut(field).clear();
            }
        });
    }

    fn page_shortcuts(&mut self, ui: &mut egui::Ui) {
        card(ui, |ui| {
            title(ui, "Raccourcis globaux");
            hint(ui, "Actifs même quand FaceLock est en arrière-plan. Ctrl / Alt / Maj / Win + une touche.");
            ui.add_space(4.0);
            self.hotkey_row(ui, "Démarrer / suspendre la surveillance", HkField::Toggle);
            self.hotkey_row(ui, "Réinitialiser la vérification", HkField::Reset);
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                title(ui, "Panic button");
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.checkbox(&mut self.cfg.panic.enabled, "Activé");
                });
            });
            hint(ui, "Exécution instantanée de tes actions, sans passer par la détection.");
            self.hotkey_row(ui, "Raccourci du panic button", HkField::Panic);
            ui.add_space(4.0);
            let mut list = std::mem::take(&mut self.cfg.panic.actions);
            self.actions_editor(ui, &mut list, ListId::Panic);
            self.cfg.panic.actions = list;
        });
        ui.add_space(8.0);
        card(ui, |ui| {
            title(ui, "Notifications Windows");
            ui.checkbox(&mut self.cfg.notifications.toggle, "Démarrer / suspendre par raccourci");
            ui.checkbox(&mut self.cfg.notifications.reset, "Réinitialisation par raccourci");
            ui.checkbox(&mut self.cfg.notifications.panic, "Panic button");
        });
    }

    fn picker_modal(&mut self, ctx: &egui::Context) {
        let Some(p) = self.picker.as_mut() else { return };
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
        let resp = egui::Modal::new(egui::Id::unique("picker")).show(ctx, |ui| {
            ui.set_width(420.0);
            title(ui, "Choisir un processus");
            let edit = ui.add(egui::TextEdit::singleline(&mut p.search).hint_text("Rechercher…").desired_width(f32::INFINITY));
            if p.focus {
                edit.request_focus();
                p.focus = false;
            }
            let needle = p.search.to_lowercase();
            egui::ScrollArea::vertical().max_height(440.0).auto_shrink([false, false]).show(ui, |ui| {
                if p.rx.is_some() {
                    hint(ui, "Chargement…");
                }
                for (name, tex) in p.items.iter().filter(|(n, _)| needle.is_empty() || n.to_lowercase().contains(&needle)) {
                    let clicked = ui
                        .horizontal(|ui| {
                            match tex {
                                Some(t) => {
                                    ui.image(egui::load::SizedTexture::new(t.id(), Vec2::splat(20.0)));
                                }
                                None => {
                                    ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                                }
                            }
                            ui.add(egui::Button::selectable(false, name.as_str()).min_size(Vec2::new(ui.available_width(), 24.0)))
                                .clicked()
                        })
                        .inner;
                    if clicked {
                        chosen = Some(name.clone());
                    }
                }
            });
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Actualiser").clicked() {
                    p.items.clear();
                }
                if ui.button("Fermer").clicked() {
                    chosen = Some(String::new());
                }
            });
        });
        if resp.should_close() && chosen.is_none() {
            chosen = Some(String::new());
        }
        if let Some(name) = chosen {
            let (list, i) = p.target;
            self.picker = None;
            if !name.is_empty() {
                let actions = match list {
                    ListId::Absence => &mut self.cfg.triggers.absence.actions,
                    ListId::Stranger => &mut self.cfg.triggers.stranger.actions,
                    ListId::Panic => &mut self.cfg.panic.actions,
                };
                if let Some(a) = actions.get_mut(i) {
                    a.params.process = name;
                }
            }
        }
    }

    fn delete_modal(&mut self, ctx: &egui::Context) {
        let Some(name) = self.confirm_delete.clone() else { return };
        let mut close = false;
        let resp = egui::Modal::new(egui::Id::unique("delete")).show(ctx, |ui| {
            title(ui, "Supprimer le profil");
            ui.label(format!("Supprimer définitivement le profil « {name} » ?"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if colored_button(ui, "Supprimer", RED.gamma_multiply(0.7), true).clicked() {
                    let path = crate::config::faces_dir().join(format!("{name}.face"));
                    match std::fs::remove_file(path) {
                        Ok(()) => {
                            self.send(Cmd::ReloadProfiles);
                            self.toast(format!("Profil « {name} » supprimé"), true);
                        }
                        Err(e) => self.toast(format!("Suppression impossible : {e}"), false),
                    }
                    close = true;
                }
                if ui.button("Annuler").clicked() {
                    close = true;
                }
            });
        });
        if close || resp.should_close() {
            self.confirm_delete = None;
            self.refresh_profiles();
        }
    }
}
