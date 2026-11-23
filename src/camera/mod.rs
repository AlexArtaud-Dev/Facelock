//! Capture webcam à la demande. API commune :
//! `init_thread()`, `list_cameras()`, `Camera::open(index, nom, taille)`,
//! `Camera::read(fresh)` -> `Frame` BGRA. La caméra est libérée au drop.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::*;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

/// YUYV (YUV 4:2:2 entrelacé, BT.601) -> BGRA.
#[allow(dead_code)]
pub(crate) fn yuyv_to_bgra(src: &[u8], w: usize, h: usize, stride: usize, out: &mut [u8]) {
    let clamp = |v: i32| v.clamp(0, 255) as u8;
    for y in 0..h {
        let row = &src[y * stride..];
        for x in (0..w).step_by(2) {
            let i = x * 2;
            if i + 3 >= row.len() {
                break;
            }
            let (y0, u, y1, v) = (
                row[i] as i32,
                row[i + 1] as i32 - 128,
                row[i + 2] as i32,
                row[i + 3] as i32 - 128,
            );
            for (dx, yy) in [(0, y0), (1, y1)] {
                if x + dx >= w {
                    break;
                }
                let c = 298 * (yy - 16);
                let o = (y * w + x + dx) * 4;
                out[o] = clamp((c + 516 * u + 128) >> 8);
                out[o + 1] = clamp((c - 100 * u - 208 * v + 128) >> 8);
                out[o + 2] = clamp((c + 409 * v + 128) >> 8);
                out[o + 3] = 255;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn yuyv_grey_and_red() {
        // Gris moyen : Y=126, U=V=128 -> ~128,128,128
        let src = [126u8, 128, 126, 128];
        let mut out = [0u8; 8];
        super::yuyv_to_bgra(&src, 2, 1, 4, &mut out);
        assert!(out[..3].iter().all(|&c| (125..=131).contains(&c)), "{out:?}");
        // Rouge BT.601 : Y=81, U=90, V=240
        let src = [81u8, 90, 81, 240];
        super::yuyv_to_bgra(&src, 2, 1, 4, &mut out);
        assert!(out[2] > 240 && out[1] < 15 && out[0] < 15, "{out:?}");
    }
}
