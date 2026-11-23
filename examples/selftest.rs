//! Vérifie le moteur sur des images : détections + empreintes (JSON sur stdout).
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

fn main() {
    let mut e = engine::Engine::new().unwrap();
    let mut out = Vec::new();
    for path in std::env::args().skip(1) {
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
