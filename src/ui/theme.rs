//! Système de design : couleurs, polices, widgets peints à la main.

use eframe::egui::{
    self, text::LayoutJob, Align2, Color32, CornerRadius, FontFamily, FontId, Response, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use std::sync::Arc;

// Thème « liquid glass » sombre : fond profond + halos colorés, surfaces
// translucides (blanc à faible opacité, couleurs prémultipliées).
pub const BG: Color32 = Color32::from_rgb(7, 8, 13);
pub const SIDEBAR: Color32 = Color32::from_rgba_premultiplied(5, 6, 10, 110);
pub const SURFACE: Color32 = Color32::from_rgba_premultiplied(10, 11, 14, 14);
pub const SURFACE2: Color32 = Color32::from_rgba_premultiplied(16, 17, 21, 20);
pub const SURFACE3: Color32 = Color32::from_rgba_premultiplied(28, 30, 36, 32);
pub const BORDER: Color32 = Color32::from_rgba_premultiplied(26, 27, 32, 26);
pub const SHEEN: Color32 = Color32::from_rgba_premultiplied(60, 60, 64, 60);
pub const SUNKEN: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 110);
pub const TEXT: Color32 = Color32::from_rgb(238, 240, 247);
pub const MUTED: Color32 = Color32::from_rgb(150, 156, 175);
pub const FAINT: Color32 = Color32::from_rgb(98, 105, 124);
pub const ACCENT: Color32 = Color32::from_rgb(112, 118, 255);
pub const VIOLET: Color32 = Color32::from_rgb(168, 112, 255);
pub const GREEN: Color32 = Color32::from_rgb(52, 222, 150);
pub const RED: Color32 = Color32::from_rgb(255, 88, 112);
pub const ORANGE: Color32 = Color32::from_rgb(255, 184, 76);
pub const TEAL: Color32 = Color32::from_rgb(34, 211, 238);

// Glyphes de la police d'icônes Phosphor (MIT, embarquée : assets/fonts)
pub const I_DASH: char = '\u{E2C2}';
pub const I_FLOW: char = '\u{E2DE}';
pub const I_FACE: char = '\u{E6FC}';
pub const I_SETTINGS: char = '\u{E270}';
pub const I_LOCK: char = '\u{E2FA}';
pub const I_EYE: char = '\u{E220}';
pub const I_PAUSE: char = '\u{E39E}';
pub const I_PLAY: char = '\u{E3D0}';
pub const I_ADD: char = '\u{E3D4}';
pub const I_DELETE: char = '\u{E4A6}';
pub const I_COPY: char = '\u{E1CA}';
pub const I_REFRESH: char = '\u{E036}';
pub const I_VIDEO: char = '\u{E4DA}';
pub const I_CLOCK: char = '\u{E19A}';
pub const I_KEYBOARD: char = '\u{E2D8}';
pub const I_CHECK: char = '\u{E182}';
pub const I_WARN: char = '\u{E4E0}';
pub const I_FOLDER: char = '\u{E24A}';
pub const I_POWER: char = '\u{E3DA}';
pub const I_CHEVRON: char = '\u{E13A}';
pub const I_CLOSE: char = '\u{E4F6}';
pub const I_BELL: char = '\u{E0CE}';
pub const I_INFO: char = '\u{E2CE}';
pub const I_LEFT: char = '\u{E138}';
pub const I_RIGHT: char = '\u{E13A}';

pub fn icons() -> FontFamily {
    FontFamily::Name("icons".into())
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

pub fn icon(c: char, size: f32) -> RichText {
    RichText::new(c.to_string()).family(icons()).size(size)
}

pub fn h1(text: &str) -> RichText {
    RichText::new(text).family(semibold()).size(24.0).color(TEXT)
}

pub fn h2(text: &str) -> RichText {
    RichText::new(text).family(semibold()).size(16.5).color(TEXT)
}

pub fn muted(text: &str) -> RichText {
    RichText::new(text).color(MUTED)
}

pub fn small(text: &str) -> RichText {
    RichText::new(text).size(12.5).color(MUTED)
}

static PHOSPHOR: &[u8] = include_bytes!("../../assets/fonts/Phosphor.ttf");

/// Premier fichier de police lisible parmi des candidats (chemins absolus).
fn read_font(candidates: &[&str]) -> Option<Vec<u8>> {
    candidates.iter().find_map(|p| std::fs::read(p).ok())
}

#[cfg(windows)]
const FONTS_BODY: &[&str] = &[r"C:\Windows\Fonts\segoeui.ttf"];
#[cfg(windows)]
const FONTS_SEMIBOLD: &[&str] = &[r"C:\Windows\Fonts\seguisb.ttf", r"C:\Windows\Fonts\segoeuib.ttf"];
#[cfg(windows)]
const FONTS_SYMBOL: &[&str] = &[r"C:\Windows\Fonts\seguisym.ttf"];
#[cfg(windows)]
const FONTS_MONO: &[&str] = &[r"C:\Windows\Fonts\consola.ttf"];

#[cfg(not(windows))]
const FONTS_BODY: &[&str] = &[
    "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
    "/usr/share/fonts/opentype/inter/Inter-Regular.otf",
    "/usr/share/fonts/inter/Inter-Regular.otf",
    "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/google-noto/NotoSans-Regular.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
#[cfg(not(windows))]
const FONTS_SEMIBOLD: &[&str] = &[
    "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf",
    "/usr/share/fonts/opentype/inter/Inter-SemiBold.otf",
    "/usr/share/fonts/inter/Inter-SemiBold.otf",
    "/usr/share/fonts/truetype/noto/NotoSans-SemiBold.ttf",
    "/usr/share/fonts/noto/NotoSans-SemiBold.ttf",
    "/usr/share/fonts/truetype/noto/NotoSans-Bold.ttf",
    "/usr/share/fonts/truetype/ubuntu/Ubuntu-M.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf",
    "/usr/share/fonts/TTF/DejaVuSans-Bold.ttf",
];
#[cfg(not(windows))]
const FONTS_SYMBOL: &[&str] = &[
    "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
    "/usr/share/fonts/TTF/DejaVuSans.ttf",
];
#[cfg(not(windows))]
const FONTS_MONO: &[&str] = &[
    "/usr/share/fonts/truetype/jetbrains-mono/JetBrainsMono-Regular.ttf",
    "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
    "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
];

pub fn setup(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let mut add = |key: &str, bytes: Option<Vec<u8>>| -> bool {
        match bytes {
            Some(b) => {
                fonts
                    .font_data
                    .insert(key.into(), Arc::new(egui::FontData::from_owned(b)));
                true
            }
            None => false,
        }
    };
    let has_ui = add("body", read_font(FONTS_BODY));
    let has_sb = add("semibold", read_font(FONTS_SEMIBOLD));
    let has_sym = add("symbols", read_font(FONTS_SYMBOL));
    let has_mono = add("mono", read_font(FONTS_MONO));
    add("phosphor", Some(PHOSPHOR.to_vec()));
    let prop = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut body = Vec::new();
    if has_ui {
        body.push("body".to_string());
    }
    body.extend(prop);
    if has_sym {
        body.push("symbols".into());
    }
    // Les icônes servent aussi de secours dans le texte courant.
    body.push("phosphor".into());
    let mut sb = Vec::new();
    if has_sb {
        sb.push("semibold".to_string());
    }
    sb.extend(body.clone());
    let mut ic = vec!["phosphor".to_string()];
    ic.extend(body.clone());
    fonts.families.insert(FontFamily::Proportional, body);
    fonts.families.insert(semibold(), sb);
    fonts.families.insert(icons(), ic);
    if has_mono {
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".into());
    }
    ctx.set_fonts(fonts);

    ctx.set_theme(egui::Theme::Dark);
    ctx.global_style_mut(|s| {
        s.spacing.item_spacing = Vec2::new(10.0, 8.0);
        s.spacing.button_padding = Vec2::new(12.0, 6.0);
        s.spacing.interact_size.y = 30.0;
        s.spacing.slider_width = 260.0;
        s.spacing.combo_width = 260.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(14.5));
        s.text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(14.5));
        s.text_styles
            .insert(egui::TextStyle::Heading, FontId::new(22.0, semibold()));
        s.text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(12.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(12.5));
        let v = &mut s.visuals;
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(16);
        v.extreme_bg_color = SUNKEN;
        v.faint_bg_color = SURFACE2;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.55);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = ACCENT;
        v.override_text_color = Some(TEXT);
        for w in [
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = CornerRadius::same(8);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.inactive.weak_bg_fill = SURFACE2;
        v.widgets.inactive.bg_fill = SURFACE3;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, BORDER);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.weak_bg_fill = SURFACE3;
        v.widgets.hovered.bg_fill = SURFACE3;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
        v.widgets.hovered.fg_stroke = Stroke::new(1.5, TEXT);
        v.widgets.active.weak_bg_fill = SURFACE3;
        v.widgets.active.bg_fill = ACCENT;
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.open.weak_bg_fill = SURFACE3;
        v.slider_trailing_fill = true;
    });
}

// ------------------------------------------------------------- conteneurs

pub fn glass_frame(radius: u8) -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(radius)
        .shadow(egui::epaint::Shadow {
            offset: [0, 10],
            blur: 34,
            spread: 0,
            color: Color32::from_black_alpha(70),
        })
}

/// Reflet spéculaire en haut d'un panneau de verre.
pub fn sheen(p: &egui::Painter, rect: egui::Rect, radius: f32) {
    let y = rect.top() + 0.75;
    let (l, r) = (rect.left() + radius * 0.8, rect.right() - radius * 0.8);
    let mut mesh = egui::Mesh::default();
    let steps = 24u32;
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        let a = (1.0 - (t * 2.0 - 1.0).abs()).powf(0.7);
        let x = egui::lerp(l..=r, t);
        let c = SHEEN.gamma_multiply(a);
        mesh.colored_vertex(egui::pos2(x, y - 0.6), c);
        mesh.colored_vertex(egui::pos2(x, y + 0.6), c);
        if i > 0 {
            let k = i * 2;
            mesh.add_triangle(k - 2, k - 1, k);
            mesh.add_triangle(k - 1, k, k + 1);
        }
    }
    p.add(mesh);
}

pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let shown = glass_frame(20).inner_margin(22).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui)
    });
    sheen(ui.painter(), shown.response.rect, 20.0);
    shown.inner
}

/// Fond de fenêtre : base profonde + halos lumineux (dégradés radiaux).
pub fn backdrop(ctx: &egui::Context) {
    let rect = ctx.content_rect();
    let p = ctx.layer_painter(egui::LayerId::background());
    p.rect_filled(rect, 0.0, BG);
    let (w, h) = (rect.width(), rect.height());
    for (fx, fy, r, color, a) in [
        (0.08, 0.02, 0.62, Color32::from_rgb(116, 66, 250), 0.42),
        (0.95, 0.95, 0.60, Color32::from_rgb(13, 170, 210), 0.30),
        (0.70, 0.35, 0.42, Color32::from_rgb(56, 90, 250), 0.22),
        (0.20, 0.90, 0.38, Color32::from_rgb(170, 60, 220), 0.14),
    ] {
        radial(
            &p,
            rect.min + Vec2::new(fx * w, fy * h),
            r * w.max(h),
            color.gamma_multiply(a),
        );
    }
}

fn radial(p: &egui::Painter, c: egui::Pos2, r: f32, color: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(c, color);
    let n = 64u32;
    let rings = [(0.35f32, 0.62f32), (0.7, 0.25), (1.0, 0.0)];
    for (ri, (rr, a)) in rings.iter().enumerate() {
        let ri = ri as u32;
        for i in 0..n {
            let ang = i as f32 / n as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(c + Vec2::new(ang.cos(), ang.sin()) * r * *rr, color.gamma_multiply(*a));
            let cur = 1 + ri * n + i;
            let next = 1 + ri * n + (i + 1) % n;
            if ri == 0 {
                mesh.add_triangle(0, cur, next);
            } else {
                mesh.add_triangle(cur - n, cur, next);
                mesh.add_triangle(cur - n, next, next - n);
            }
        }
    }
    p.add(mesh);
}

pub fn inset<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(SURFACE2)
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(14)
        .inner_margin(egui::Margin::symmetric(16, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui)
        })
        .inner
}

/// Ligne de réglage façon Windows 11 : icône, titre, description, contrôle à droite.
pub fn setting_row(ui: &mut egui::Ui, ic: char, title: &str, desc: &str, control: impl FnOnce(&mut egui::Ui)) {
    inset(ui, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(2.0);
            ui.label(icon(ic, 18.0).color(MUTED));
            ui.add_space(6.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(title).color(TEXT));
                if !desc.is_empty() {
                    ui.label(small(desc));
                }
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), control);
        });
    });
}

pub fn page_header(ui: &mut egui::Ui, title: &str, subtitle: &str, right: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal(|ui| {
        // Le texte s'arrête avant la zone des boutons (pas de chevauchement).
        let text_w = (ui.available_width() - 250.0).max(200.0);
        ui.vertical(|ui| {
            ui.set_max_width(text_w);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.label(h1(title));
            ui.add(egui::Label::new(muted(subtitle)).wrap());
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), right);
    });
    ui.add_space(14.0);
}

// ---------------------------------------------------------------- widgets

pub fn one_line(ui: &egui::Ui, text: &str, font: FontId, color: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_string(), font, color);
    job.wrap.max_width = max_width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    ui.painter().layout_job(job)
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Ghost,
    Danger,
    Tint(Color32),
}

/// Bouton peint : icône optionnelle + texte.
pub fn button(ui: &mut egui::Ui, ic: Option<char>, text: &str, kind: Kind, enabled: bool) -> Response {
    let font = FontId::new(14.0, semibold());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, TEXT);
    let icon_w = if ic.is_some() { 24.0 } else { 0.0 };
    let pad = if text.is_empty() { 9.0 } else { 16.0 };
    let w = galley.size().x + icon_w + pad * 2.0 - if text.is_empty() { 8.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(w.max(34.0), 34.0),
        if enabled { Sense::click() } else { Sense::hover() },
    );
    if ui.is_rect_visible(rect) {
        let hovered = enabled && resp.hovered();
        let pressed = enabled && resp.is_pointer_button_down_on();
        let (fill, fg, stroke) = match kind {
            Kind::Primary => (
                if hovered { ACCENT.gamma_multiply(1.15) } else { ACCENT },
                Color32::WHITE,
                Stroke::NONE,
            ),
            Kind::Ghost => (
                if hovered { SURFACE3 } else { SURFACE2 },
                TEXT,
                Stroke::new(1.0, BORDER),
            ),
            Kind::Danger => (
                if hovered {
                    RED.gamma_multiply(0.35)
                } else {
                    RED.gamma_multiply(0.18)
                },
                RED,
                Stroke::NONE,
            ),
            Kind::Tint(c) => (
                if hovered {
                    c.gamma_multiply(0.38)
                } else {
                    c.gamma_multiply(0.22)
                },
                c,
                Stroke::NONE,
            ),
        };
        let fill = if pressed { fill.gamma_multiply(0.85) } else { fill };
        let alpha = if enabled { 1.0 } else { 0.4 };
        let p = ui.painter();
        p.rect(rect, 10.0, fill.gamma_multiply(alpha), stroke, StrokeKind::Inside);
        if kind == Kind::Primary {
            // Brillance « verre » sur la moitié haute
            let top = egui::Rect::from_min_max(rect.min, egui::pos2(rect.right(), rect.center().y));
            p.rect_filled(
                top,
                CornerRadius {
                    nw: 10,
                    ne: 10,
                    sw: 0,
                    se: 0,
                },
                Color32::from_white_alpha(if enabled { 26 } else { 10 }),
            );
            sheen(p, rect, 10.0);
        }
        let fg = fg.gamma_multiply(alpha);
        let mut x = rect.left() + pad - if text.is_empty() { 4.0 } else { 0.0 };
        if let Some(c) = ic {
            p.text(
                egui::pos2(x + 8.0, rect.center().y),
                Align2::CENTER_CENTER,
                c,
                FontId::new(15.0, icons()),
                fg,
            );
            x += icon_w;
        }
        if !text.is_empty() {
            p.galley_with_override_text_color(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
        }
    }
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn icon_button(ui: &mut egui::Ui, ic: char, tip: &str, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(rect, 8.0, SURFACE3);
    }
    let c = if resp.hovered() {
        color
    } else {
        color.gamma_multiply(0.75)
    };
    ui.painter()
        .text(rect.center(), Align2::CENTER_CENTER, ic, FontId::new(15.0, icons()), c);
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Interrupteur animé.
pub fn toggle(ui: &mut egui::Ui, on: &mut bool) -> Response {
    let size = Vec2::new(44.0, 24.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let t = ui.ctx().animate_bool_with_time(resp.id, *on, 0.12);
    let p = ui.painter();
    let off_c = SURFACE3;
    let fill = lerp_color(off_c, ACCENT, t);
    p.rect(
        rect,
        12.0,
        fill,
        Stroke::new(1.0, if *on { Color32::TRANSPARENT } else { BORDER }),
        StrokeKind::Inside,
    );
    let x = egui::lerp(rect.left() + 12.0..=rect.right() - 12.0, t);
    p.circle_filled(
        egui::pos2(x, rect.center().y),
        8.0,
        if *on { Color32::WHITE } else { MUTED },
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Interpolation sur les composantes prémultipliées (fonctionne avec les surfaces translucides).
pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_premultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

/// Puce sélectionnable (contrôle segmenté).
pub fn chip(ui: &mut egui::Ui, selected: bool, ic: Option<char>, text: &str, color: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(13.5), TEXT);
    let iw = if ic.is_some() { 22.0 } else { 0.0 };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + iw + 26.0, 32.0), Sense::click());
    let p = ui.painter();
    let (fill, stroke, fg) = if selected {
        (color.gamma_multiply(0.2), Stroke::new(1.5, color), TEXT)
    } else if resp.hovered() {
        (SURFACE3, Stroke::new(1.0, BORDER), TEXT)
    } else {
        (SURFACE2, Stroke::new(1.0, BORDER), MUTED)
    };
    p.rect(rect, 16.0, fill, stroke, StrokeKind::Inside);
    let mut x = rect.left() + 13.0;
    if let Some(c) = ic {
        p.text(
            egui::pos2(x + 7.0, rect.center().y),
            Align2::CENTER_CENTER,
            c,
            FontId::new(13.0, icons()),
            if selected { color } else { fg },
        );
        x += iw;
    }
    p.galley_with_override_text_color(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, fg);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::new(11.5, semibold()), color);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 6.0), Sense::hover());
    ui.painter().rect_filled(rect, 10.0, color.gamma_multiply(0.16));
    ui.painter().galley(rect.min + Vec2::new(8.0, 3.0), galley, color);
}

/// Disque coloré avec un glyphe au centre.
pub fn icon_disc(p: &egui::Painter, center: egui::Pos2, radius: f32, color: Color32, ic: char) {
    p.circle_filled(center, radius, color.gamma_multiply(0.18));
    p.text(
        center,
        Align2::CENTER_CENTER,
        ic,
        FontId::new(radius * 0.95, icons()),
        color,
    );
}

/// Combinaison affichée en touches de clavier.
pub fn keycaps(ui: &mut egui::Ui, combo: &str) {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (i, part) in crate::hotkey::pretty(combo).split(" + ").enumerate() {
        if i > 0 {
            ui.label(small("+"));
        }
        let galley = ui
            .painter()
            .layout_no_wrap(part.to_string(), FontId::new(13.0, semibold()), TEXT);
        let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::new(16.0, 10.0), Sense::hover());
        let p = ui.painter();
        p.rect_filled(rect.translate(Vec2::new(0.0, 2.0)), 6.0, SUNKEN);
        p.rect(rect, 6.0, SURFACE3, Stroke::new(1.0, BORDER), StrokeKind::Inside);
        p.galley(rect.min + Vec2::new(8.0, 5.0), galley, TEXT);
    }
}

pub fn status_dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(12.0), Sense::hover());
    ui.painter()
        .circle_filled(rect.center(), 7.0, color.gamma_multiply(0.25));
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

// ------------------------------------------------------- durées

/// Paliers naturels des curseurs de durée (secondes).
pub const STEPS_GRACE: &[f64] = &[
    0.0, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0, 600.0,
];
pub const STEPS_PRESENCE: &[f64] = &[0.0, 1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0, 60.0];
pub const STEPS_RETURN: &[f64] = &[
    1.0, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0, 600.0,
];
pub const STEPS_COOLDOWN: &[f64] = &[
    0.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0, 90.0, 120.0, 180.0, 300.0, 600.0,
];
pub const STEPS_WAIT: &[f64] = &[0.5, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0, 15.0, 20.0, 30.0, 45.0, 60.0];
pub const STEPS_INTERVAL: &[f64] = &[0.2, 0.3, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0, 5.0, 10.0];

/// « 0,5 s », « 45 s », « 2 min », « 1 min 30 s ».
pub fn fmt_duration(s: f64) -> String {
    let num = |v: f64| {
        if (v - v.round()).abs() < 0.01 {
            format!("{}", v.round() as i64)
        } else {
            format!("{v:.2}").trim_end_matches('0').replace('.', ",")
        }
    };
    if s < 60.0 {
        return format!("{} s", num(s));
    }
    let (m, r) = ((s / 60.0).floor(), s % 60.0);
    if r < 0.5 {
        format!("{} min", m as i64)
    } else {
        format!("{} min {} s", m as i64, r.round() as i64)
    }
}

/// Accepte « 90 », « 90 s », « 1,5 », « 2 min », « 1 min 30 », « 1:30 ».
pub fn parse_duration(text: &str) -> Option<f64> {
    let t = text.trim().to_lowercase().replace(',', ".");
    if let Some((m, s)) = t.split_once(':') {
        return Some(m.trim().parse::<f64>().ok()? * 60.0 + s.trim().parse::<f64>().ok()?);
    }
    if let Some((m, rest)) = t.split_once("min") {
        let rest = rest.trim().trim_end_matches('s').trim();
        let secs = if rest.is_empty() {
            0.0
        } else {
            rest.parse::<f64>().ok()?
        };
        return Some(m.trim().parse::<f64>().ok()? * 60.0 + secs);
    }
    t.trim_end_matches('s').trim().parse().ok()
}

/// Curseur sur des paliers naturels + valeur exacte modifiable.
/// Remplace les curseurs logarithmiques (2 s se retrouvait aux deux tiers).
pub fn duration_slider(ui: &mut egui::Ui, value: &mut f64, steps: &[f64]) -> Response {
    let (min, max) = (steps[0], steps[steps.len() - 1]);
    let mut idx = steps
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - *value).abs().total_cmp(&(b.1 - *value).abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let slider = ui.add(egui::Slider::new(&mut idx, 0..=steps.len() - 1).show_value(false));
    if slider.changed() {
        *value = steps[idx];
    }
    let exact = ui.add(
        egui::DragValue::new(value)
            .range(min..=max)
            .speed(0.25)
            .custom_formatter(|v, _| fmt_duration(v))
            .custom_parser(parse_duration),
    );
    slider | exact
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_roundtrip() {
        assert_eq!(fmt_duration(0.5), "0,5 s");
        assert_eq!(fmt_duration(2.0), "2 s");
        assert_eq!(fmt_duration(120.0), "2 min");
        assert_eq!(fmt_duration(90.0), "1 min 30 s");
        assert_eq!(parse_duration("90"), Some(90.0));
        assert_eq!(parse_duration("1,5 s"), Some(1.5));
        assert_eq!(parse_duration("2 min"), Some(120.0));
        assert_eq!(parse_duration("1 min 30 s"), Some(90.0));
        assert_eq!(parse_duration("1:30"), Some(90.0));
        for s in [0.0, 0.5, 3.0, 45.0, 60.0, 90.0, 600.0] {
            assert_eq!(parse_duration(&fmt_duration(s)), Some(s), "{s}");
        }
    }
}
