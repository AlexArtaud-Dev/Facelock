//! Écran de confidentialité : fenêtre plein écran, au premier plan, opaque,
//! qui masque tout le bureau avec un message ou une image.
//!
//! Fermeture : raccourci de secours (obligatoire), écouté à la fois ici et
//! globalement par le service — qui tue ce processus même s'il ne répond
//! plus —, retour d'un visage autorisé, ou `facelock --dismiss`.

use super::theme::{self, *};
use crate::daemon::OverlaySpec;
use eframe::egui::{self, Align2, Color32, FontId, Vec2};
use std::sync::Arc;
use std::time::{Duration, Instant};

const DEFAULT_TITLE: &str = "Accès non autorisé";
const DEFAULT_TEXT: &str = "Ce poste est protégé par FaceLock.\nVous n'êtes pas autorisé à consulter cet écran.";

pub fn run(spec: OverlaySpec) {
    let icon = egui::IconData {
        rgba: crate::icon::rgba(64, crate::icon::Variant::Active),
        width: 64,
        height: 64,
    };
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("FaceLock — écran de confidentialité")
            .with_fullscreen(true)
            .with_decorations(false)
            .with_window_level(egui::WindowLevel::AlwaysOnTop)
            .with_taskbar(false)
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    let _ = eframe::run_native(
        "FaceLock overlay",
        opts,
        Box::new(move |cc| {
            theme::setup(&cc.egui_ctx);
            Ok(Box::new(Overlay::new(cc, spec)))
        }),
    );
}

struct Overlay {
    title: String,
    text: String,
    image: Option<egui::TextureHandle>,
    logo: egui::TextureHandle,
    /// (modificateurs FaceLock, code VK) du raccourci de secours
    combo: Option<(u32, u32)>,
    last_focus: Instant,
    sized: bool,
}

/// Charge une image (PNG, JPEG, WebP) et la réduit si elle est énorme.
fn load_image(path: &str) -> Option<egui::ColorImage> {
    let img = image::open(path).ok()?;
    let img = if img.width() > 3840 || img.height() > 2160 {
        img.thumbnail(3840, 2160)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

impl Overlay {
    fn new(cc: &eframe::CreationContext<'_>, spec: OverlaySpec) -> Self {
        let ctx = &cc.egui_ctx;
        let px = crate::icon::rgba(256, crate::icon::Variant::Active);
        let logo = ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([256, 256], &px),
            Default::default(),
        );
        let image = (!spec.image.is_empty())
            .then(|| load_image(&spec.image))
            .flatten()
            .map(|img| ctx.load_texture("privacy-image", img, egui::TextureOptions::LINEAR));
        // Texte libre : première ligne = titre, le reste = sous-titre.
        let (title, text) = match spec.text.split_once('\n') {
            Some((t, rest)) => (t.trim().to_string(), rest.trim().to_string()),
            None if !spec.text.is_empty() => (spec.text.clone(), String::new()),
            None => (DEFAULT_TITLE.into(), DEFAULT_TEXT.into()),
        };
        Overlay {
            title,
            text,
            image,
            logo,
            combo: crate::hotkey::parse_combo(&spec.hotkey).ok(),
            last_focus: Instant::now(),
            sized: false,
        }
    }

    /// Le raccourci de secours a-t-il été pressé dans cette fenêtre ?
    fn combo_pressed(&self, ctx: &egui::Context) -> bool {
        let Some((mods, vk)) = self.combo else { return false };
        ctx.input(|i| {
            i.events.iter().any(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    let Some(k) = super::egui_key_name(*key).and_then(|n| crate::hotkey::vk_from_name(&n)) else {
                        return false;
                    };
                    let m = (modifiers.ctrl as u32 * crate::hotkey::MOD_CONTROL)
                        | (modifiers.alt as u32 * crate::hotkey::MOD_ALT)
                        | (modifiers.shift as u32 * crate::hotkey::MOD_SHIFT);
                    // La touche Windows n'est pas visible ici : le service s'en charge.
                    k == vk && m == (mods & !crate::hotkey::MOD_WIN)
                }
                _ => false,
            })
        })
    }
}

impl eframe::App for Overlay {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.combo_pressed(&ctx) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Reste au premier plan et garde le clavier (raccourci local).
        if self.last_focus.elapsed() > Duration::from_secs(3) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            self.last_focus = Instant::now();
        }
        ctx.request_repaint_after(Duration::from_secs(3));
        ctx.set_cursor_icon(egui::CursorIcon::None);
        // Filet de sécurité si le gestionnaire de fenêtres ignore le plein écran :
        // la fenêtre est étendue à tout le moniteur.
        if !self.sized {
            let (monitor, inner) = ctx.input(|i| (i.viewport().monitor_size, i.viewport().inner_rect));
            if let (Some(m), Some(r)) = (monitor, inner) {
                if r.width() + 1.0 < m.x || r.height() + 1.0 < m.y {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(m));
                }
                self.sized = true;
            } else {
                ctx.request_repaint_after(Duration::from_millis(100));
            }
        }

        theme::backdrop(&ctx);
        let rect = ctx.content_rect();
        let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("overlay")));
        // Voile supplémentaire : rien du bureau ne doit transparaître.
        p.rect_filled(rect, 0.0, Color32::from_rgba_premultiplied(4, 5, 9, 200));

        let center = rect.center();
        match &self.image {
            Some(tex) => {
                let size = tex.size_vec2();
                let max = rect.size() * Vec2::new(0.86, if self.text.is_empty() { 0.82 } else { 0.68 });
                let scale = (max.x / size.x).min(max.y / size.y).min(4.0);
                let img_rect =
                    egui::Rect::from_center_size(center - Vec2::new(0.0, rect.height() * 0.04), size * scale);
                p.add(
                    egui::epaint::RectShape::filled(img_rect, 18.0, Color32::WHITE).with_texture(
                        tex.id(),
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    ),
                );
                if !self.title.is_empty() && self.title != DEFAULT_TITLE {
                    p.text(
                        egui::pos2(center.x, img_rect.bottom() + 40.0),
                        Align2::CENTER_TOP,
                        &self.title,
                        FontId::new(34.0, semibold()),
                        TEXT,
                    );
                }
            }
            None => {
                let s = (rect.height() / 1080.0).clamp(0.6, 2.0);
                let top = center.y - 230.0 * s;
                // Halo + logo
                let lc = egui::pos2(center.x, top + 80.0 * s);
                p.circle_filled(lc, 120.0 * s, ACCENT.gamma_multiply(0.10));
                p.circle_filled(lc, 90.0 * s, ACCENT.gamma_multiply(0.10));
                p.image(
                    self.logo.id(),
                    egui::Rect::from_center_size(lc, Vec2::splat(140.0 * s)),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                p.text(
                    egui::pos2(center.x, top + 210.0 * s),
                    Align2::CENTER_TOP,
                    &self.title,
                    FontId::new(64.0 * s, semibold()),
                    TEXT,
                );
                for (i, line) in self.text.lines().enumerate() {
                    p.text(
                        egui::pos2(center.x, top + (310.0 + 34.0 * i as f32) * s),
                        Align2::CENTER_TOP,
                        line,
                        FontId::proportional(24.0 * s),
                        MUTED,
                    );
                }
            }
        }
        let footer = p.text(
            egui::pos2(center.x + 12.0, rect.bottom() - 40.0),
            Align2::CENTER_BOTTOM,
            "FaceLock · écran de confidentialité",
            FontId::proportional(15.0),
            FAINT,
        );
        p.text(
            egui::pos2(footer.left() - 14.0, footer.center().y),
            Align2::CENTER_CENTER,
            I_LOCK,
            FontId::new(15.0, icons()),
            FAINT,
        );
    }
}
