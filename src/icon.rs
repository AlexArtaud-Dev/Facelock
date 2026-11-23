//! Logo FaceLock, rastérisé à la volée (antialiasé, net à toute taille) :
//! squircle en dégradé violet -> cyan avec reflet « verre », cadre de scan
//! facial (4 coins) et trou de serrure au centre.

#[derive(Clone, Copy, PartialEq)]
pub enum Variant {
    Active,
    Suspended,
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Distance signée à un segment épais (capsule).
fn capsule(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32, r: f32) -> bool {
    let (vx, vy) = (bx - ax, by - ay);
    let t = (((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
    let (dx, dy) = (px - ax - vx * t, py - ay - vy * t);
    dx * dx + dy * dy <= r * r
}

/// Couleur (RGB linéaire 0..1) + alpha d'un point en coordonnées [-1, 1].
fn shade(x: f32, y: f32, v: Variant, detail: bool) -> ([f32; 3], f32) {
    // Squircle (superellipse n = 5)
    let n = 5.0;
    let s = (x.abs() / 0.94).powf(n) + (y.abs() / 0.94).powf(n);
    if s > 1.0 {
        return ([0.0; 3], 0.0);
    }
    let (c0, c1, c2) = match v {
        Variant::Active => ([0.45, 0.26, 0.98], [0.22, 0.38, 0.98], [0.05, 0.74, 0.88]),
        Variant::Suspended => ([0.36, 0.38, 0.44], [0.42, 0.44, 0.50], [0.56, 0.58, 0.64]),
    };
    let t = ((x + y) * 0.5 + 1.0) * 0.5; // diagonale haut-gauche -> bas-droite
    let mut c = if t < 0.5 {
        mix(c0, c1, t * 2.0)
    } else {
        mix(c1, c2, (t - 0.5) * 2.0)
    };
    // Assombrissement vers le bas (profondeur)
    let dark = 1.0 - 0.22 * smooth(-0.2, 1.0, y);
    c = [c[0] * dark, c[1] * dark, c[2] * dark];
    // Reflet de verre : ellipse lumineuse dans la moitié haute
    let ex = x / 1.15;
    let ey = (y + 0.78) / 0.72;
    let gloss = (1.0 - (ex * ex + ey * ey)).clamp(0.0, 1.0);
    let g = 0.20 * smooth(0.0, 0.6, gloss) * (1.0 - smooth(-0.1, 0.15, y));
    c = mix(c, [1.0; 3], g);
    // Liseré lumineux le long du bord (plus fort en haut)
    let rim = smooth(0.80, 0.97, s) * (0.45 - 0.35 * (y + 1.0) * 0.5);
    c = mix(c, [1.0; 3], rim.max(0.0));

    // Pictogramme blanc, avec une ombre portée douce
    if !glyph(x, y, detail) && glyph(x, y - 0.045, detail) {
        c = [c[0] * 0.72, c[1] * 0.72, c[2] * 0.78];
    }
    if glyph(x, y, detail) {
        let glow = 1.0 - 0.08 * smooth(-0.3, 0.4, y);
        c = [glow, glow, glow];
    }
    (c, 1.0)
}

fn glyph(x: f32, y: f32, detail: bool) -> bool {
    let mut white = false;
    let k = if detail { 1.0 } else { 1.35 };
    // Trou de serrure
    let (kx, ky) = (x / k, y / k);
    if kx * kx + (ky + 0.12).powi(2) <= 0.165f32.powi(2) {
        white = true;
    }
    if (-0.06..=0.30).contains(&ky) {
        let hw = 0.055 + (ky + 0.06) / 0.36 * 0.06;
        if kx.abs() <= hw {
            white = true;
        }
    }
    // Cadre de scan (coins), seulement si la taille le permet
    if detail {
        let (a, l, r) = (0.56, 0.22, 0.05);
        for (sx, sy) in [(-1.0f32, -1.0f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let (cx, cy) = (sx * a, sy * a);
            if capsule(x, y, cx, cy, cx - sx * l, cy, r) || capsule(x, y, cx, cy, cx, cy - sy * l, r) {
                white = true;
            }
        }
    }
    white
}

/// RGBA non prémultiplié, `size` x `size`.
pub fn rgba(size: usize, v: Variant) -> Vec<u8> {
    const SS: usize = 4;
    let detail = size >= 28;
    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let mut acc = [0f32; 3];
            let mut alpha = 0f32;
            for sy in 0..SS {
                for sx in 0..SS {
                    let x = ((px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32) * 2.0 - 1.0;
                    let y = ((py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32) * 2.0 - 1.0;
                    let (c, a) = shade(x, y, v, detail);
                    for i in 0..3 {
                        acc[i] += c[i] * a;
                    }
                    alpha += a;
                }
            }
            let i = (py * size + px) * 4;
            if alpha > 0.0 {
                for c in 0..3 {
                    out[i + c] = (acc[c] / alpha * 255.0).round().clamp(0.0, 255.0) as u8;
                }
                out[i + 3] = (alpha / (SS * SS) as f32 * 255.0).round() as u8;
            }
        }
    }
    out
}
