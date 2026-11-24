//! Éditeur visuel de workflows : « QUAND » -> « ALORS » -> « PUIS »…
//! Chaque bloc est cliquable ; l'inspecteur sous le schéma édite le bloc choisi.

use super::theme::*;
use crate::actions::{self, KINDS};
use crate::config::{action, ActionConf, StrangerWhen, Trigger, Workflow};
use eframe::egui::{self, Align2, Color32, FontId, Sense, Stroke, StrokeKind, Vec2};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Sel {
    Trigger,
    Action(usize),
}

/// Demandes remontées par l'éditeur (exécutées par l'application).
pub enum Op {
    Delete(usize),
    Duplicate(usize),
    Test(u64),
    Browse(u64, usize),
    Record(u64),
    /// Raccourci de secours d'un écran de confidentialité (workflow, action)
    RecordAction(u64, usize),
    /// Choisir l'image d'un écran de confidentialité (explorateur de fichiers)
    PickImage(u64, usize),
}

pub struct TriggerMeta {
    pub kind: &'static str,
    pub label: &'static str,
    pub icon: char,
    pub color: Color32,
}

pub const TRIGGERS: &[TriggerMeta] = &[
    TriggerMeta {
        kind: "absence",
        label: "Je m'absente",
        icon: '\u{E73A}',
        color: ACCENT,
    },
    TriggerMeta {
        kind: "stranger",
        label: "Visage inconnu",
        icon: '\u{EC36}',
        color: RED,
    },
    TriggerMeta {
        kind: "return",
        label: "Je reviens",
        icon: '\u{E580}',
        color: GREEN,
    },
    TriggerMeta {
        kind: "hotkey",
        label: "Raccourci clavier",
        icon: '\u{E2D8}',
        color: VIOLET,
    },
];

pub fn trigger_meta(t: &Trigger) -> &'static TriggerMeta {
    TRIGGERS.iter().find(|m| m.kind == t.kind()).unwrap_or(&TRIGGERS[0])
}

fn secs(s: f64) -> String {
    if s < 1.0 {
        "immédiatement".into()
    } else if s >= 60.0 && (s % 60.0).abs() < 0.5 {
        format!("après {} min", (s / 60.0).round())
    } else {
        format!("après {s:.0} s")
    }
}

pub fn trigger_summary(t: &Trigger) -> String {
    match t {
        Trigger::Absence { grace } => format!("Personne d'autorisé, {}", secs(*grace)),
        Trigger::Stranger { grace, when } => {
            let w = match when {
                StrangerWhen::Always => "toujours",
                StrangerWhen::Alone => "si je suis absent",
                StrangerWhen::WithMe => "derrière moi",
            };
            if *grace >= 1.0 {
                format!("{w}, {}", secs(*grace))
            } else {
                w.to_string()
            }
        }
        Trigger::Return { min_absence } => format!("après ≥ {min_absence:.0} s d'absence"),
        Trigger::Hotkey { combo } if combo.is_empty() => "à définir".into(),
        Trigger::Hotkey { combo } => crate::hotkey::pretty(combo),
    }
}

pub fn action_color(id: &str) -> Color32 {
    match id {
        "lock" | "sleep" | "screen_off" => ORANGE,
        "switch_desktop" | "show_desktop" | "move_app" | "minimize_app" | "kill_app" => TEAL,
        "media_pause" | "mute" => ACCENT,
        _ => MUTED,
    }
}

pub fn action_summary(a: &ActionConf) -> String {
    if let Some(problem) = a.problem() {
        return format!("⚠ {problem}");
    }
    let k = actions::kind(&a.action);
    let p = &a.params;
    let proc_ = if p.process.trim().is_empty() {
        "appli à choisir".to_string()
    } else {
        p.process.clone()
    };
    match a.action.as_str() {
        "switch_desktop" => format!("Bureau {}", p.desktop.unwrap_or(1)),
        "move_app" => format!("{proc_} → bureau {}", p.desktop.unwrap_or(2)),
        "minimize_app" | "kill_app" if !p.title.trim().is_empty() => {
            format!("{proc_} · « {} »", p.title.trim())
        }
        "minimize_app" | "kill_app" => proc_,
        "notify" if !p.text.trim().is_empty() => format!("« {} »", p.text.trim()),
        "wait" => format!("{:.0} s", p.seconds.unwrap_or(1.0)),
        "privacy_screen" if !p.image.trim().is_empty() => format!(
            "Image · {}",
            std::path::Path::new(p.image.trim())
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        "privacy_screen" => p
            .text
            .lines()
            .next()
            .filter(|l| !l.trim().is_empty())
            .unwrap_or("Accès non autorisé")
            .to_string(),
        _ => k.desc.to_string(),
    }
}

// ------------------------------------------------------------- nœuds

const NODE: Vec2 = Vec2::new(212.0, 70.0);

fn node(
    ui: &mut egui::Ui,
    caption: &str,
    title: &str,
    sub: &str,
    ic: char,
    color: Color32,
    selected: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(NODE, Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hovered = resp.hovered();
    let p = ui.painter();
    let fill = if selected {
        lerp_color(SURFACE2, color, 0.12)
    } else if hovered {
        SURFACE3
    } else {
        SURFACE2
    };
    let stroke = if selected {
        Stroke::new(2.0, color)
    } else {
        Stroke::new(1.0, if hovered { color.gamma_multiply(0.6) } else { BORDER })
    };
    p.rect(rect, 14.0, fill, stroke, StrokeKind::Inside);
    let c = egui::pos2(rect.left() + 32.0, rect.center().y);
    icon_disc(p, c, 19.0, color, ic);
    let x = rect.left() + 60.0;
    let w = rect.right() - x - 10.0;
    let cap = one_line(ui, caption, FontId::new(10.5, semibold()), color, w);
    let t = one_line(ui, title, FontId::new(14.0, semibold()), TEXT, w);
    let sub_color = if sub.starts_with('⚠') { RED } else { MUTED };
    let s = one_line(ui, sub, FontId::proportional(12.0), sub_color, w);
    let p = ui.painter();
    p.galley(egui::pos2(x, rect.top() + 11.0), cap, color);
    p.galley(egui::pos2(x, rect.top() + 26.0), t, TEXT);
    p.galley(egui::pos2(x, rect.top() + 46.0), s, sub_color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn arrow(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(30.0, NODE.y), Sense::hover());
    let p = ui.painter();
    let y = rect.center().y;
    p.line_segment(
        [egui::pos2(rect.left() + 2.0, y), egui::pos2(rect.right() - 6.0, y)],
        Stroke::new(2.0, FAINT),
    );
    let tip = egui::pos2(rect.right() - 2.0, y);
    p.add(egui::Shape::convex_polygon(
        vec![tip, egui::pos2(tip.x - 8.0, y - 5.0), egui::pos2(tip.x - 8.0, y + 5.0)],
        FAINT,
        Stroke::NONE,
    ));
}

fn add_node(ui: &mut egui::Ui) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(70.0, NODE.y), Sense::click());
    let p = ui.painter();
    let c = if resp.hovered() { ACCENT } else { FAINT };
    let r = rect.shrink2(Vec2::new(4.0, 6.0));
    p.rect(
        r,
        14.0,
        if resp.hovered() {
            ACCENT.gamma_multiply(0.08)
        } else {
            Color32::TRANSPARENT
        },
        Stroke::new(1.5, c),
        StrokeKind::Inside,
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        I_ADD,
        FontId::new(18.0, icons()),
        c,
    );
    resp.on_hover_text("Ajouter une action")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Schéma miniature (galerie de modèles) : icônes reliées.
pub fn mini_flow(ui: &mut egui::Ui, wf: &Workflow) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let m = trigger_meta(&wf.trigger);
        let disc = |ui: &mut egui::Ui, color: Color32, ic: char| {
            let (rect, _) = ui.allocate_exact_size(Vec2::splat(28.0), Sense::hover());
            icon_disc(ui.painter(), rect.center(), 14.0, color, ic);
        };
        disc(ui, m.color, m.icon);
        for a in &wf.actions {
            ui.label(icon(I_CHEVRON, 10.0).color(FAINT));
            disc(ui, action_color(&a.action), actions::kind(&a.action).icon);
        }
    });
}

// -------------------------------------------------------- carte workflow

pub struct Ctx<'a> {
    pub sel: &'a mut Option<(u64, Sel)>,
    pub ops: &'a mut Vec<Op>,
    pub recording: Option<u64>,
    pub recording_action: Option<(u64, usize)>,
    /// Raccourcis en conflit : usage -> message explicite
    pub conflicts: &'a [(crate::config::HotkeyUse, String)],
}

pub fn workflow_card(ui: &mut egui::Ui, idx: usize, wf: &mut Workflow, cx: &mut Ctx) {
    let meta = trigger_meta(&wf.trigger);
    let strip_color = if wf.enabled { meta.color } else { FAINT };
    let shown = glass_frame(20)
        .inner_margin(egui::Margin {
            left: 20,
            right: 14,
            top: 14,
            bottom: 16,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            // En-tête : nom, type, interrupteur, actions
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut wf.name)
                        .font(FontId::new(17.0, semibold()))
                        .frame(egui::Frame::NONE)
                        .desired_width(280.0),
                )
                .on_hover_text("Renommer");
                badge(
                    ui,
                    if wf.enabled { "Actif" } else { "En pause" },
                    if wf.enabled { GREEN } else { MUTED },
                );
                if cx.conflicts.iter().any(|(u, _)| u.workflow() == Some(wf.id)) {
                    badge(ui, "Conflit", RED);
                } else if wf.actions.iter().any(|a| a.problem().is_some()) {
                    badge(ui, "Incomplet", ORANGE);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    toggle(ui, &mut wf.enabled).on_hover_text(if wf.enabled { "Désactiver" } else { "Activer" });
                    ui.add_space(6.0);
                    if icon_button(ui, I_DELETE, "Supprimer", RED).clicked() {
                        cx.ops.push(Op::Delete(idx));
                    }
                    if icon_button(ui, I_COPY, "Dupliquer", MUTED).clicked() {
                        cx.ops.push(Op::Duplicate(idx));
                    }
                    if icon_button(ui, I_PLAY, "Tester maintenant (exécute les actions)", GREEN).clicked() {
                        cx.ops.push(Op::Test(wf.id));
                    }
                });
            });
            ui.add_space(8.0);

            // Schéma
            let sel_here = cx.sel.filter(|(id, _)| *id == wf.id).map(|(_, s)| s);
            ui.scope(|ui| {
                if !wf.enabled {
                    ui.multiply_opacity(0.5);
                }
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(0.0, 10.0);
                    let r = node(
                        ui,
                        "QUAND",
                        meta.label,
                        &conflict_or(
                            cx,
                            &crate::config::HotkeyUse::Trigger(wf.id),
                            trigger_summary(&wf.trigger),
                        ),
                        meta.icon,
                        meta.color,
                        sel_here == Some(Sel::Trigger),
                    );
                    if r.clicked() {
                        *cx.sel = toggle_sel(*cx.sel, wf.id, Sel::Trigger);
                    }
                    for (i, a) in wf.actions.iter().enumerate() {
                        arrow(ui);
                        let k = actions::kind(&a.action);
                        let cap = if i == 0 { "ALORS" } else { "PUIS" };
                        let r = node(
                            ui,
                            cap,
                            k.label,
                            &conflict_or(cx, &crate::config::HotkeyUse::Dismiss(wf.id, i), action_summary(a)),
                            k.icon,
                            action_color(&a.action),
                            sel_here == Some(Sel::Action(i)),
                        );
                        if r.clicked() {
                            *cx.sel = toggle_sel(*cx.sel, wf.id, Sel::Action(i));
                        }
                    }
                    arrow(ui);
                    if add_node(ui).clicked() {
                        wf.actions.push(action("lock"));
                        *cx.sel = Some((wf.id, Sel::Action(wf.actions.len() - 1)));
                    }
                });
            });

            // Inspecteur
            if let Some(s) = sel_here {
                ui.add_space(12.0);
                inspector(ui, wf, s, cx);
            }
        });
    // Liseré coloré à gauche, à la hauteur exacte de la carte
    let r = shown.response.rect;
    let strip = egui::Rect::from_min_max(
        egui::pos2(r.left() + 0.5, r.top() + 18.0),
        egui::pos2(r.left() + 3.5, r.bottom() - 18.0),
    );
    ui.painter().rect_filled(
        strip.expand2(Vec2::new(3.0, 0.0)),
        4.0,
        strip_color.gamma_multiply(0.18),
    );
    ui.painter().rect_filled(strip, 2.0, strip_color);
    sheen(ui.painter(), r, 20.0);
}

/// Sous-titre d'un bloc : l'avertissement de conflit prime.
fn conflict_or(cx: &Ctx, u: &crate::config::HotkeyUse, normal: String) -> String {
    if cx.conflicts.iter().any(|(c, _)| c == u) {
        "⚠ raccourci en conflit".into()
    } else {
        normal
    }
}

/// Message de conflit affiché sous un champ de raccourci.
fn conflict_line(ui: &mut egui::Ui, cx: &Ctx, u: &crate::config::HotkeyUse) {
    if let Some((_, msg)) = cx.conflicts.iter().find(|(c, _)| c == u) {
        ui.horizontal(|ui| {
            field_label(ui, "");
            ui.label(egui::RichText::new(format!("⚠ {msg}")).color(RED));
        });
    }
}

fn toggle_sel(cur: Option<(u64, Sel)>, id: u64, s: Sel) -> Option<(u64, Sel)> {
    if cur == Some((id, s)) {
        None
    } else {
        Some((id, s))
    }
}

fn inspector(ui: &mut egui::Ui, wf: &mut Workflow, s: Sel, cx: &mut Ctx) {
    egui::Frame::new()
        .fill(SUNKEN.gamma_multiply(0.6))
        .stroke(Stroke::new(1.0, BORDER))
        .corner_radius(16)
        .inner_margin(16)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            match s {
                Sel::Trigger => trigger_editor(ui, wf, cx),
                Sel::Action(i) if i < wf.actions.len() => action_editor(ui, wf, i, cx),
                Sel::Action(_) => {
                    *cx.sel = None;
                }
            }
        });
}

fn field_label(ui: &mut egui::Ui, text: &str) {
    ui.add_sized([150.0, 30.0], egui::Label::new(muted(text)));
}

fn trigger_editor(ui: &mut egui::Ui, wf: &mut Workflow, cx: &mut Ctx) {
    ui.label(small("DÉCLENCHEUR").color(MUTED));
    ui.horizontal_wrapped(|ui| {
        for m in TRIGGERS {
            if chip(ui, wf.trigger.kind() == m.kind, Some(m.icon), m.label, m.color).clicked()
                && wf.trigger.kind() != m.kind
            {
                wf.trigger = Trigger::default_of(m.kind);
            }
        }
    });
    ui.add_space(8.0);
    match &mut wf.trigger {
        Trigger::Absence { grace } => {
            ui.label(muted(
                "Se déclenche quand aucun visage enrôlé n'est devant l'écran pendant le délai choisi.",
            ));
            ui.horizontal(|ui| {
                field_label(ui, "Délai de grâce");
                duration_slider(ui, grace, STEPS_GRACE);
            });
        }
        Trigger::Stranger { grace, when } => {
            ui.label(muted("Se déclenche quand un visage non enrôlé est détecté."));
            ui.horizontal(|ui| {
                field_label(ui, "Condition");
                for (w, l) in [
                    (StrangerWhen::Always, "Toujours"),
                    (StrangerWhen::Alone, "Quand je suis absent"),
                    (StrangerWhen::WithMe, "Derrière moi"),
                ] {
                    if chip(ui, *when == w, None, l, RED).clicked() {
                        *when = w;
                    }
                }
            });
            ui.horizontal(|ui| {
                field_label(ui, "Présence continue");
                duration_slider(ui, grace, STEPS_PRESENCE);
            });
        }
        Trigger::Return { min_absence } => {
            ui.label(muted(
                "Se déclenche quand ton visage réapparaît après une absence (sans verrouillage entre-temps).",
            ));
            ui.horizontal(|ui| {
                field_label(ui, "Absence minimale");
                duration_slider(ui, min_absence, STEPS_RETURN);
            });
        }
        Trigger::Hotkey { combo } => {
            ui.label(muted(
                "Exécution instantanée, sans passer par la caméra (idéal pour un panic button).",
            ));
            ui.horizontal(|ui| {
                field_label(ui, "Raccourci");
                let recording = cx.recording == Some(wf.id);
                if recording {
                    ui.label(egui::RichText::new("Appuie sur ta combinaison…").color(ORANGE));
                } else if combo.is_empty() {
                    ui.label(muted("non défini"));
                } else {
                    keycaps(ui, combo);
                }
                ui.add_space(8.0);
                if button(
                    ui,
                    Some(I_KEYBOARD),
                    if combo.is_empty() { "Enregistrer" } else { "Modifier" },
                    Kind::Ghost,
                    cx.recording.is_none(),
                )
                .clicked()
                {
                    cx.ops.push(Op::Record(wf.id));
                }
                if !combo.is_empty() && icon_button(ui, I_CLOSE, "Effacer", MUTED).clicked() {
                    combo.clear();
                }
            });
            conflict_line(ui, cx, &crate::config::HotkeyUse::Trigger(wf.id));
        }
    }
    if !matches!(wf.trigger, Trigger::Absence { .. }) {
        ui.horizontal(|ui| {
            field_label(ui, "Anti-répétition");
            duration_slider(ui, &mut wf.cooldown, STEPS_COOLDOWN);
        });
    }
}

fn action_editor(ui: &mut egui::Ui, wf: &mut Workflow, i: usize, cx: &mut Ctx) {
    let n = wf.actions.len();
    ui.horizontal(|ui| {
        ui.label(small(&format!("ACTION {} / {n}", i + 1)).color(MUTED));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if button(ui, Some(I_DELETE), "Retirer", Kind::Danger, true).clicked() {
                wf.actions.remove(i);
                *cx.sel = None;
            }
            if icon_button(ui, I_RIGHT, "Déplacer après", MUTED).clicked() && i + 1 < n {
                wf.actions.swap(i, i + 1);
                *cx.sel = Some((wf.id, Sel::Action(i + 1)));
            }
            if icon_button(ui, I_LEFT, "Déplacer avant", MUTED).clicked() && i > 0 {
                wf.actions.swap(i - 1, i);
                *cx.sel = Some((wf.id, Sel::Action(i - 1)));
            }
        });
    });
    if i >= wf.actions.len() {
        return;
    }
    let wf_id = wf.id;
    let a = &mut wf.actions[i];
    // Grille des types d'action
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(8.0, 8.0);
        for k in KINDS {
            if chip(ui, a.action == k.id, Some(k.icon), k.label, action_color(k.id))
                .on_hover_text(k.desc)
                .clicked()
            {
                a.action = k.id.to_string();
            }
        }
    });
    let k = actions::kind(&a.action);
    if k.process || k.desktop || k.text || k.seconds {
        ui.add_space(8.0);
    }
    if k.process {
        ui.horizontal(|ui| {
            field_label(ui, "Application");
            ui.add(
                egui::TextEdit::singleline(&mut a.params.process)
                    .hint_text("ex. chrome.exe")
                    .desired_width(200.0),
            );
            if button(ui, Some(I_FOLDER), "Choisir…", Kind::Ghost, true).clicked() {
                cx.ops.push(Op::Browse(wf.id, i));
            }
        });
        ui.horizontal(|ui| {
            field_label(ui, "Titre contient");
            ui.add(
                egui::TextEdit::singleline(&mut a.params.title)
                    .hint_text("optionnel, ex. YouTube")
                    .desired_width(200.0),
            );
        });
    }
    if k.desktop {
        ui.horizontal(|ui| {
            field_label(ui, "Bureau virtuel");
            let def = if a.action == "move_app" { 2 } else { 1 };
            let mut d = a.params.desktop.unwrap_or(def);
            for n in 1..=4u32 {
                if chip(ui, d == n, None, &n.to_string(), TEAL).clicked() {
                    d = n;
                }
            }
            ui.add(egui::DragValue::new(&mut d).range(1..=20).prefix("n° "));
            if a.params.desktop != Some(d) {
                a.params.desktop = Some(d);
            }
        });
    }
    if k.text {
        ui.horizontal(|ui| {
            field_label(ui, "Message");
            ui.add(
                egui::TextEdit::singleline(&mut a.params.text)
                    .hint_text("Workflow déclenché")
                    .desired_width(320.0),
            );
        });
    }
    if k.seconds {
        ui.horizontal(|ui| {
            field_label(ui, "Durée");
            let mut s = a.params.seconds.unwrap_or(1.0);
            duration_slider(ui, &mut s, STEPS_WAIT);
            if a.params.seconds != Some(s) {
                a.params.seconds = Some(s);
            }
        });
    }
    if k.overlay {
        overlay_editor(ui, a, wf_id, i, cx);
    }
}

/// Réglages de l'écran de confidentialité (raccourci de secours obligatoire).
fn overlay_editor(ui: &mut egui::Ui, a: &mut ActionConf, wf_id: u64, i: usize, cx: &mut Ctx) {
    ui.horizontal_top(|ui| {
        field_label(ui, "Message");
        ui.add(
            egui::TextEdit::multiline(&mut a.params.text)
                .hint_text("Accès non autorisé\nCe poste est protégé : vous n'êtes pas autorisé à le consulter.")
                .desired_rows(2)
                .desired_width(380.0),
        );
    });
    ui.horizontal(|ui| {
        field_label(ui, "Image (optionnel)");
        ui.add(
            egui::TextEdit::singleline(&mut a.params.image)
                .hint_text("chemin d'une image PNG, JPEG ou WebP")
                .desired_width(300.0),
        );
        if button(ui, Some(I_FOLDER), "Parcourir…", Kind::Ghost, true).clicked() {
            cx.ops.push(Op::PickImage(wf_id, i));
        }
        if !a.params.image.is_empty() && icon_button(ui, I_CLOSE, "Retirer l'image", MUTED).clicked() {
            a.params.image.clear();
        }
    });
    ui.horizontal(|ui| {
        field_label(ui, "Raccourci de secours");
        let recording = cx.recording_action == Some((wf_id, i));
        if recording {
            ui.label(egui::RichText::new("Appuie sur ta combinaison…").color(ORANGE));
        } else if a.params.hotkey.trim().is_empty() {
            ui.label(egui::RichText::new("obligatoire").color(RED));
        } else {
            keycaps(ui, &a.params.hotkey);
        }
        ui.add_space(8.0);
        let label = if a.params.hotkey.is_empty() {
            "Enregistrer"
        } else {
            "Modifier"
        };
        if button(ui, Some(I_KEYBOARD), label, Kind::Ghost, cx.recording.is_none()).clicked() {
            cx.ops.push(Op::RecordAction(wf_id, i));
        }
    });
    conflict_line(ui, cx, &crate::config::HotkeyUse::Dismiss(wf_id, i));
    ui.horizontal_top(|ui| {
        field_label(ui, "");
        // Passe à la ligne au lieu d'élargir l'inspecteur.
        ui.vertical(|ui| {
            ui.set_max_width((ui.available_width() - 8.0).max(200.0));
            ui.add(
                egui::Label::new(small(
                    "Ferme l'écran à tout moment, même s'il ne répond plus. Choisis une combinaison peu courante (ex. Ctrl + Alt + Maj + U).",
                ))
                .wrap(),
            );
        });
    });
    ui.horizontal(|ui| {
        field_label(ui, "Fermeture auto");
        let mut c = a.params.closes_on_return();
        if toggle(ui, &mut c).changed() {
            a.params.close_on_return = Some(c);
        }
        ui.label(muted("dès que ton visage est reconnu"));
    });
}

// ------------------------------------------------------------ modèles

pub struct Preset {
    pub title: &'static str,
    pub desc: &'static str,
    pub build: fn() -> Workflow,
}

fn wf(name: &str, trigger: Trigger, actions: Vec<ActionConf>, cooldown: f64) -> Workflow {
    Workflow {
        name: name.into(),
        trigger,
        actions,
        cooldown,
        ..Workflow::default()
    }
}

fn with(id: &str, f: impl FnOnce(&mut crate::config::Params)) -> ActionConf {
    let mut a = action(id);
    f(&mut a.params);
    a
}

pub const PRESETS: &[Preset] = &[
    Preset {
        title: "Écran de confidentialité",
        desc: "Un inconnu regarde pendant ton absence : écran plein « Accès non autorisé ».",
        build: || {
            wf(
                "Écran de confidentialité",
                Trigger::Stranger {
                    grace: 0.0,
                    when: StrangerWhen::Alone,
                },
                vec![action("privacy_screen")],
                10.0,
            )
        },
    },
    Preset {
        title: "Verrouillage automatique",
        desc: "Tu t'éloignes : la session se verrouille après 30 s.",
        build: || {
            wf(
                "Verrouillage automatique",
                Trigger::Absence { grace: 30.0 },
                vec![action("lock")],
                0.0,
            )
        },
    },
    Preset {
        title: "Anti-regard indiscret",
        desc: "Quelqu'un regarde par-dessus ton épaule : bureau affiché et alerte.",
        build: || {
            wf(
                "Anti-regard indiscret",
                Trigger::Stranger {
                    grace: 1.0,
                    when: StrangerWhen::WithMe,
                },
                vec![
                    action("show_desktop"),
                    with("notify", |p| p.text = "Quelqu'un regarde ton écran".into()),
                ],
                30.0,
            )
        },
    },
    Preset {
        title: "Intrus au poste",
        desc: "Un inconnu s'installe pendant ton absence : verrouillage immédiat.",
        build: || {
            wf(
                "Intrus au poste",
                Trigger::Stranger {
                    grace: 0.0,
                    when: StrangerWhen::Alone,
                },
                vec![action("lock")],
                20.0,
            )
        },
    },
    Preset {
        title: "Panic button",
        desc: "Un raccourci : bureau 2 et son coupé, instantanément.",
        build: || {
            wf(
                "Panic button",
                Trigger::Hotkey { combo: String::new() },
                vec![with("switch_desktop", |p| p.desktop = Some(2)), action("mute")],
                0.0,
            )
        },
    },
    Preset {
        title: "Pause café",
        desc: "Absent 20 s : la musique se met en pause et l'écran s'éteint.",
        build: || {
            wf(
                "Pause café",
                Trigger::Absence { grace: 20.0 },
                vec![action("media_pause"), action("screen_off")],
                0.0,
            )
        },
    },
    Preset {
        title: "Retour au poste",
        desc: "Ton visage revient : retour au bureau 1 et petit mot d'accueil.",
        build: || {
            wf(
                "Retour au poste",
                Trigger::Return { min_absence: 10.0 },
                vec![
                    with("switch_desktop", |p| p.desktop = Some(1)),
                    with("notify", |p| p.text = "Bon retour !".into()),
                ],
                10.0,
            )
        },
    },
    Preset {
        title: "Économie d'énergie",
        desc: "Absent 10 minutes : le PC se met en veille.",
        build: || {
            wf(
                "Économie d'énergie",
                Trigger::Absence { grace: 600.0 },
                vec![action("sleep")],
                0.0,
            )
        },
    },
    Preset {
        title: "Workflow vide",
        desc: "Pars de zéro et construis ta propre chaîne.",
        build: || {
            wf(
                "Mon workflow",
                Trigger::Absence { grace: 30.0 },
                vec![action("lock")],
                20.0,
            )
        },
    },
];
