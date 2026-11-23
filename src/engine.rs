//! Détection (YuNet) + reconnaissance (SFace), inférence 100 % Rust via tract.
//!
//! Le pré/post-traitement reproduit celui d'OpenCV (FaceDetectorYN /
//! FaceRecognizerSF) : mêmes modèles, mêmes seuils, mêmes empreintes.

use std::path::Path;
use std::sync::Arc;
use tract_onnx::prelude::*;

static YUNET: &[u8] = include_bytes!("../models/yunet.onnx");
static SFACE: &[u8] = include_bytes!("../models/sface.onnx");

pub const EMB: usize = 128;
const SCORE_THRESHOLD: f32 = 0.8;
const NMS_THRESHOLD: f32 = 0.3;
const DET_WIDTH: usize = 320;
const FACE_MAGIC: &[u8; 4] = b"FLK1";

type Plan = Arc<TypedRunnableModel>;

/// Image BGRA 8 bits, lignes contiguës (stride = w * 4).
pub struct Frame {
    pub w: usize,
    pub h: usize,
    pub bgra: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct Face {
    /// x, y, w, h en pixels de la frame d'origine
    pub bbox: [f32; 4],
    /// 5 points (yeux, nez, coins de la bouche) : x0,y0,x1,y1...
    pub landmarks: [f32; 10],
    pub score: f32,
}

pub struct Profile {
    pub name: String,
    pub embeddings: Vec<[f32; EMB]>,
}

pub struct Engine {
    detector: Option<(usize, usize, Plan)>,
    recognizer: Plan,
    pub profiles: Vec<Profile>,
}

fn load_detector(pw: usize, ph: usize) -> TractResult<Plan> {
    let mut model = tract_onnx::onnx().model_for_read(&mut &YUNET[..])?;
    // Le modèle embarque des formes intermédiaires figées en 640x640 :
    // on les efface pour l'exécuter à la taille réellement utilisée.
    for node in model.nodes.iter_mut() {
        for out in node.outputs.iter_mut() {
            out.fact = InferenceFact::default();
        }
    }
    model
        .with_input_fact(0, f32::fact([1, 3, ph, pw]).into())?
        .into_optimized()?
        .into_runnable()
}

fn load_recognizer() -> TractResult<Plan> {
    tract_onnx::onnx()
        .model_for_read(&mut &SFACE[..])?
        .with_input_fact(0, f32::fact([1, 3, 112, 112]).into())?
        .into_optimized()?
        .into_runnable()
}

#[inline]
fn sample_bilinear(f: &Frame, x: f32, y: f32, out: &mut [f32; 3]) {
    // Bordure constante noire (comme warpAffine BORDER_CONSTANT).
    if x <= -1.0 || y <= -1.0 || x >= f.w as f32 || y >= f.h as f32 {
        *out = [0.0; 3];
        return;
    }
    let x0 = x.floor();
    let y0 = y.floor();
    let fx = x - x0;
    let fy = y - y0;
    let (x0, y0) = (x0 as isize, y0 as isize);
    let px = |xx: isize, yy: isize, c: usize| -> f32 {
        if xx < 0 || yy < 0 || xx >= f.w as isize || yy >= f.h as isize {
            0.0
        } else {
            f.bgra[(yy as usize * f.w + xx as usize) * 4 + c] as f32
        }
    };
    for (c, o) in out.iter_mut().enumerate() {
        let a = px(x0, y0, c) * (1.0 - fx) + px(x0 + 1, y0, c) * fx;
        let b = px(x0, y0 + 1, c) * (1.0 - fx) + px(x0 + 1, y0 + 1, c) * fx;
        *o = a * (1.0 - fy) + b * fy;
    }
}

fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x1 = a[0].max(b[0]);
    let y1 = a[1].max(b[1]);
    let x2 = (a[0] + a[2]).min(b[0] + b[2]);
    let y2 = (a[1] + a[3]).min(b[1] + b[3]);
    let inter = (x2 - x1).max(0.0) * (y2 - y1).max(0.0);
    let union = a[2] * a[3] + b[2] * b[3] - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

pub fn box_iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    iou(a, b)
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

impl Engine {
    pub fn new() -> TractResult<Self> {
        let mut e = Engine {
            detector: None,
            recognizer: load_recognizer()?,
            profiles: Vec::new(),
        };
        e.reload_profiles();
        Ok(e)
    }

    /// Détection des visages (YuNet), coordonnées dans la frame d'origine.
    pub fn detect(&mut self, f: &Frame) -> TractResult<Vec<Face>> {
        let dw = DET_WIDTH.min(f.w);
        let dh = ((f.h * dw) as f32 / f.w as f32).round().max(32.0) as usize;
        let pw = dw.div_ceil(32) * 32;
        let ph = dh.div_ceil(32) * 32;
        if !matches!(&self.detector, Some((w, h, _)) if *w == pw && *h == ph) {
            self.detector = Some((pw, ph, load_detector(pw, ph)?));
        }
        let plan = self.detector.as_ref().unwrap().2.clone();

        // Redimensionnement bilinéaire (convention INTER_LINEAR d'OpenCV),
        // entrée planaire BGR 0..255, complétée de zéros jusqu'au multiple de 32.
        let sx = f.w as f32 / dw as f32;
        let sy = f.h as f32 / dh as f32;
        let plane = pw * ph;
        let mut input = vec![0f32; 3 * plane];
        let mut px = [0f32; 3];
        for y in 0..dh {
            let fy = ((y as f32 + 0.5) * sy - 0.5).max(0.0);
            for x in 0..dw {
                let fx = ((x as f32 + 0.5) * sx - 0.5).max(0.0);
                sample_bilinear(f, fx.min(f.w as f32 - 1.0), fy.min(f.h as f32 - 1.0), &mut px);
                let i = y * pw + x;
                input[i] = px[0];
                input[plane + i] = px[1];
                input[2 * plane + i] = px[2];
            }
        }
        let tensor = Tensor::from_shape(&[1, 3, ph, pw], &input)?;
        let outs = plan.run(tvec!(tensor.into()))?;

        let mut faces = Vec::new();
        for (i, stride) in [8usize, 16, 32].into_iter().enumerate() {
            let cls = outs[i].try_as_plain_ram()?.as_slice::<f32>()?;
            let obj = outs[3 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            let bbox = outs[6 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            let kps = outs[9 + i].try_as_plain_ram()?.as_slice::<f32>()?;
            let cols = pw / stride;
            let rows = ph / stride;
            let s = stride as f32;
            for r in 0..rows {
                for c in 0..cols {
                    let idx = r * cols + c;
                    let score = (cls[idx].clamp(0.0, 1.0) * obj[idx].clamp(0.0, 1.0)).sqrt();
                    if score < SCORE_THRESHOLD {
                        continue;
                    }
                    let cx = (c as f32 + bbox[idx * 4]) * s;
                    let cy = (r as f32 + bbox[idx * 4 + 1]) * s;
                    let w = bbox[idx * 4 + 2].exp() * s;
                    let h = bbox[idx * 4 + 3].exp() * s;
                    let mut lm = [0f32; 10];
                    for n in 0..5 {
                        lm[2 * n] = (kps[idx * 10 + 2 * n] + c as f32) * s * sx;
                        lm[2 * n + 1] = (kps[idx * 10 + 2 * n + 1] + r as f32) * s * sy;
                    }
                    faces.push(Face {
                        bbox: [(cx - w / 2.0) * sx, (cy - h / 2.0) * sy, w * sx, h * sy],
                        landmarks: lm,
                        score,
                    });
                }
            }
        }
        // NMS
        faces.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Face> = Vec::new();
        for f in faces {
            if kept.iter().all(|k| iou(&k.bbox, &f.bbox) <= NMS_THRESHOLD) {
                kept.push(f);
            }
        }
        Ok(kept)
    }

    /// Empreinte SFace (L2-normalisée) d'un visage détecté.
    pub fn embed(&self, f: &Frame, face: &Face) -> TractResult<[f32; EMB]> {
        // Transformation de similarité (moindres carrés / Umeyama sans
        // réflexion) des 5 repères vers le gabarit 112x112 d'OpenCV.
        const DST: [[f32; 2]; 5] = [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ];
        let src: Vec<[f32; 2]> = (0..5)
            .map(|i| [face.landmarks[2 * i], face.landmarks[2 * i + 1]])
            .collect();
        let ms = src.iter().fold([0.0; 2], |a, p| [a[0] + p[0] / 5.0, a[1] + p[1] / 5.0]);
        let md = DST.iter().fold([0.0; 2], |a, p| [a[0] + p[0] / 5.0, a[1] + p[1] / 5.0]);
        let (mut num_a, mut num_b, mut den) = (0f32, 0f32, 0f32);
        for i in 0..5 {
            let (sx, sy) = (src[i][0] - ms[0], src[i][1] - ms[1]);
            let (dx, dy) = (DST[i][0] - md[0], DST[i][1] - md[1]);
            num_a += sx * dx + sy * dy;
            num_b += sx * dy - sy * dx;
            den += sx * sx + sy * sy;
        }
        if den <= f32::EPSILON {
            return Err(tract_onnx::prelude::TractError::msg("repères dégénérés"));
        }
        let (a, b) = (num_a / den, num_b / den);
        // dst = [a -b; b a] * src + t
        let tx = md[0] - (a * ms[0] - b * ms[1]);
        let ty = md[1] - (b * ms[0] + a * ms[1]);
        // Inverse : src = R^-1 (dst - t)
        let det = a * a + b * b;
        let (ia, ib) = (a / det, b / det);

        let plane = 112 * 112;
        let mut input = vec![0f32; 3 * plane];
        let mut px = [0f32; 3];
        for y in 0..112 {
            for x in 0..112 {
                let (u, v) = (x as f32 - tx, y as f32 - ty);
                let srcx = ia * u + ib * v;
                let srcy = -ib * u + ia * v;
                sample_bilinear(f, srcx, srcy, &mut px);
                let i = y * 112 + x;
                // SFace attend du RGB (swapRB=true dans blobFromImage)
                input[i] = px[2];
                input[plane + i] = px[1];
                input[2 * plane + i] = px[0];
            }
        }
        let tensor = Tensor::from_shape(&[1, 3, 112, 112], &input)?;
        let out = self.recognizer.run(tvec!(tensor.into()))?;
        let slice = out[0].try_as_plain_ram()?.as_slice::<f32>()?;
        let mut emb = [0f32; EMB];
        emb.copy_from_slice(&slice[..EMB]);
        normalize(&mut emb);
        Ok(emb)
    }

    /// Meilleur profil (nom, score cosinus). Nom = None sous le seuil.
    pub fn identify(&self, emb: &[f32; EMB], threshold: f32) -> (Option<String>, f32) {
        let mut best: (Option<&str>, f32) = (None, -1.0);
        for p in &self.profiles {
            for e in &p.embeddings {
                let s: f32 = e.iter().zip(emb.iter()).map(|(a, b)| a * b).sum();
                if s > best.1 {
                    best = (Some(&p.name), s);
                }
            }
        }
        if best.1 >= threshold {
            (best.0.map(str::to_string), best.1)
        } else {
            (None, best.1)
        }
    }

    pub fn reload_profiles(&mut self) {
        self.profiles = list_profiles()
            .into_iter()
            .filter_map(|(name, path)| {
                let embeddings = read_profile(&path)?;
                (!embeddings.is_empty()).then_some(Profile { name, embeddings })
            })
            .collect();
    }
}

pub fn list_profiles() -> Vec<(String, std::path::PathBuf)> {
    let mut out: Vec<_> = std::fs::read_dir(crate::config::faces_dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            if p.extension()? != "face" {
                return None;
            }
            Some((p.file_stem()?.to_string_lossy().to_string(), p))
        })
        .collect();
    out.sort_by_key(|(n, _)| n.to_lowercase());
    out
}

fn read_profile(path: &Path) -> Option<Vec<[f32; EMB]>> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 8 || &data[..4] != FACE_MAGIC {
        return None;
    }
    let n = u32::from_le_bytes(data[4..8].try_into().ok()?) as usize;
    let body = &data[8..];
    if body.len() != n * EMB * 4 {
        return None;
    }
    Some(
        body.as_chunks::<{ EMB * 4 }>()
            .0
            .iter()
            .map(|chunk| {
                let mut e = [0f32; EMB];
                for (v, b) in e.iter_mut().zip(chunk.as_chunks::<4>().0) {
                    *v = f32::from_le_bytes(*b);
                }
                normalize(&mut e);
                e
            })
            .collect(),
    )
}

pub fn valid_profile_name(name: &str) -> bool {
    let n = name.chars().count();
    (1..=32).contains(&n)
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == ' ' || c == '-' || c == '_')
        && !name.starts_with(' ')
        && !name.ends_with(' ')
}

pub fn write_profile(name: &str, embeddings: &[[f32; EMB]]) -> std::io::Result<()> {
    let mut data = Vec::with_capacity(8 + embeddings.len() * EMB * 4);
    data.extend_from_slice(FACE_MAGIC);
    data.extend_from_slice(&(embeddings.len() as u32).to_le_bytes());
    for e in embeddings {
        for v in e {
            data.extend_from_slice(&v.to_le_bytes());
        }
    }
    std::fs::write(crate::config::faces_dir().join(format!("{name}.face")), data)
}
