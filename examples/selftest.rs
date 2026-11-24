//! Vérifie le moteur de reconnaissance.
//!
//!   cargo run --release --example selftest                 auto-test synthétique (CI)
//!   cargo run --release --example selftest -- a.jpg b.png  détections + empreintes en JSON
#[path = "../src/config.rs"]
#[allow(dead_code)]
mod config;
#[path = "../src/engine.rs"]
#[allow(dead_code)]
mod engine;
#[path = "../src/hotkey.rs"]
#[allow(dead_code)]
mod hotkey;

fn load(path: &str) -> engine::Frame {
    let img = image::open(path).unwrap().to_rgb8();
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut bgra = Vec::with_capacity(w * h * 4);
    for p in img.pixels() {
        bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
    }
    engine::Frame { w, h, bgra }
}

/// Charge les deux modèles et les exécute sur une image synthétique :
/// la détection ne doit rien trouver et l'empreinte doit être unitaire.
fn synthetic(e: &mut engine::Engine) {
    let (w, h) = (640, 480);
    let bgra: Vec<u8> = (0..w * h)
        .flat_map(|i| [(i % w * 255 / w) as u8, (i / w * 255 / h) as u8, 128, 255])
        .collect();
    let f = engine::Frame { w, h, bgra };
    let t = std::time::Instant::now();
    let faces = e.detect(&f).expect("détection");
    println!("détection : {} visage(s) en {:?}", faces.len(), t.elapsed());
    assert!(faces.is_empty(), "aucun visage attendu sur un dégradé");
    let face = engine::Face {
        bbox: [250.0, 150.0, 140.0, 180.0],
        landmarks: [280.0, 210.0, 360.0, 210.0, 320.0, 250.0, 290.0, 290.0, 350.0, 290.0],
        score: 1.0,
    };
    let t = std::time::Instant::now();
    let emb = e.embed(&f, &face).expect("empreinte");
    let norm: f32 = emb.iter().map(|v| v * v).sum::<f32>().sqrt();
    println!(
        "empreinte : {} dimensions, norme {norm:.4}, en {:?}",
        emb.len(),
        t.elapsed()
    );
    assert!(
        emb.iter().all(|v| v.is_finite()) && (norm - 1.0).abs() < 1e-3,
        "empreinte invalide"
    );
    println!("ok");
}

fn main() {
    let mut e = engine::Engine::new().expect("chargement des modèles");
    let mut paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        return synthetic(&mut e);
    }
    // --people a.jpg … : personnes détectées (x1, y1, x2, y2, score) en JSON
    if paths[0] == "--people" {
        paths.remove(0);
        let mut out = Vec::new();
        for path in paths {
            let f = load(&path);
            let t = std::time::Instant::now();
            let people = e.detect_people(&f, 0.35).unwrap();
            eprintln!("{path}: {} personne(s) en {:?}", people.len(), t.elapsed());
            let boxes: Vec<[f32; 5]> = people
                .iter()
                .map(|p| {
                    [
                        p.bbox[0],
                        p.bbox[1],
                        p.bbox[0] + p.bbox[2],
                        p.bbox[1] + p.bbox[3],
                        p.score,
                    ]
                })
                .collect();
            out.push(serde_json::json!({"path": path, "boxes": boxes}));
        }
        println!("{}", serde_json::to_string(&out).unwrap());
        return;
    }
    let mut out = Vec::new();
    for path in paths {
        let f = load(&path);
        let t = std::time::Instant::now();
        let faces = e.detect(&f).unwrap();
        let td = t.elapsed();
        for face in &faces {
            let t = std::time::Instant::now();
            let emb = e.embed(&f, face).unwrap();
            let te = t.elapsed();
            eprintln!(
                "{path}: det {td:?} emb {te:?} box {:?} score {:.3}",
                face.bbox, face.score
            );
            out.push(serde_json::json!({"path": path, "bbox": face.bbox, "lm": face.landmarks, "score": face.score, "emb": emb.to_vec()}));
        }
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}
