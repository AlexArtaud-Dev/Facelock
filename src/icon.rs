//! Icône (cercle + cadenas) rastérisée à la volée, antialiasée, à toute taille.

/// Couleurs du disque : actif / suspendu.
pub const ACTIVE: [u8; 3] = [30, 120, 220];
pub const SUSPENDED: [u8; 3] = [120, 120, 120];

fn rounded_rect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> bool {
    let cx = x.clamp(x0 + r, x1 - r);
    let cy = y.clamp(y0 + r, y1 - r);
    (x - cx).powi(2) + (y - cy).powi(2) <= r * r && x >= x0 && x <= x1 && y >= y0 && y <= y1
}

/// RGBA non prémultiplié, `size` x `size`.
pub fn rgba(size: usize, color: [u8; 3]) -> Vec<u8> {
    const SS: usize = 4;
    let mut out = vec![0u8; size * size * 4];
    for py in 0..size {
        for px in 0..size {
            let (mut disc, mut white) = (0u32, 0u32);
            for sy in 0..SS {
                for sx in 0..SS {
                    // coordonnées normalisées sur une grille 64x64 (dessin d'origine)
                    let x = (px as f32 + (sx as f32 + 0.5) / SS as f32) / size as f32 * 64.0;
                    let y = (py as f32 + (sy as f32 + 0.5) / SS as f32) / size as f32 * 64.0;
                    let in_disc = (x - 32.0).powi(2) + (y - 32.0).powi(2) <= 30.5f32.powi(2);
                    if !in_disc {
                        continue;
                    }
                    disc += 1;
                    let body = rounded_rect(x, y, 19.0, 29.0, 45.0, 51.0, 4.0);
                    let d = ((x - 32.0).powi(2) + (y - 25.0).powi(2)).sqrt();
                    let shackle = y <= 29.5 && (7.0..=11.5).contains(&d);
                    if body || shackle {
                        white += 1;
                    }
                }
            }
            let n = (SS * SS) as f32;
            let a = disc as f32 / n;
            if a == 0.0 {
                continue;
            }
            let w = white as f32 / disc as f32;
            let i = (py * size + px) * 4;
            for c in 0..3 {
                out[i + c] = (color[c] as f32 * (1.0 - w) + 255.0 * w).round() as u8;
            }
            out[i + 3] = (a * 255.0).round() as u8;
        }
    }
    out
}
