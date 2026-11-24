//! Personnes au visage caché : une silhouette proche sans visage visible
//! compte comme une personne inconnue, sauf si c'est l'utilisateur.
//!
//! Règles :
//! - une silhouette qui contient un visage autorisé est « moi » ;
//! - une silhouette suivie en continu garde ce statut (tu tournes la tête,
//!   tu regardes ton téléphone : tu restes toi) ;
//! - une silhouette sans visage visible et sans historique devient inconnue
//!   après un court délai de confirmation (le temps qu'un visage soit vu) ;
//! - une silhouette au visage inconnu mais visible est déjà traitée par la
//!   reconnaissance faciale ; elle n'est pas comptée une deuxième fois ;
//! - les silhouettes trop petites (loin de l'écran) sont ignorées.

use crate::engine::box_iou;
use std::time::{Duration, Instant};

/// Une silhouette non revue depuis ce délai est oubliée.
pub const TRACK_KEEP: Duration = Duration::from_secs(4);
/// Durée minimale sans visage avant de compter comme inconnue.
pub const HIDDEN_CONFIRM: Duration = Duration::from_millis(2500);
/// Recouvrement minimal pour considérer que c'est la même silhouette.
const SAME_PERSON_IOU: f32 = 0.3;

#[derive(Clone, Debug)]
struct Track {
    bbox: [f32; 4],
    me: bool,
    last_seen: Instant,
    /// Depuis quand cette silhouette est vue sans visage
    hidden_since: Option<Instant>,
}

#[derive(Default)]
pub struct Presence {
    tracks: Vec<Track>,
}

/// Le centre du visage est-il dans la silhouette ?
fn contains_face(person: &[f32; 4], face: &[f32; 4]) -> bool {
    let (cx, cy) = (face[0] + face[2] / 2.0, face[1] + face[3] / 2.0);
    cx >= person[0] && cx <= person[0] + person[2] && cy >= person[1] && cy <= person[1] + person[3]
}

impl Presence {
    pub fn clear(&mut self) {
        self.tracks.clear();
    }

    /// Met à jour le suivi et renvoie les silhouettes inconnues au visage caché.
    ///
    /// - `people` : silhouettes détectées (x, y, w, h) ;
    /// - `faces` : visages détectés et s'ils sont autorisés ;
    /// - `frame_h`, `min_height` : seules les silhouettes d'au moins
    ///   `min_height` x hauteur de l'image (donc proches) comptent.
    pub fn update(
        &mut self,
        people: &[[f32; 4]],
        faces: &[([f32; 4], bool)],
        frame_h: f32,
        min_height: f32,
        now: Instant,
    ) -> Vec<[f32; 4]> {
        let mut next: Vec<Track> = Vec::new();
        let mut hidden = Vec::new();
        for p in people.iter().filter(|p| p[3] >= min_height * frame_h) {
            // Suivi : la silhouette précédente qui recouvre le plus celle-ci.
            let prev = self
                .tracks
                .iter()
                .filter(|t| now.duration_since(t.last_seen) <= TRACK_KEEP)
                .map(|t| (box_iou(&t.bbox, p), t))
                .filter(|(iou, _)| *iou > SAME_PERSON_IOU)
                .max_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(_, t)| t.clone());
            let inside: Vec<bool> = faces
                .iter()
                .filter(|(f, _)| contains_face(p, f))
                .map(|(_, known)| *known)
                .collect();
            let me = inside.contains(&true) || prev.as_ref().is_some_and(|t| t.me);
            let hidden_since = if me || !inside.is_empty() {
                None
            } else {
                Some(prev.as_ref().and_then(|t| t.hidden_since).unwrap_or(now))
            };
            if hidden_since.is_some_and(|t| now.duration_since(t) >= HIDDEN_CONFIRM) {
                hidden.push(*p);
            }
            next.push(Track {
                bbox: *p,
                me,
                last_seen: now,
                hidden_since,
            });
        }
        // Les silhouettes non revues restent un moment (détection manquée).
        for t in &self.tracks {
            let rematched = next.iter().any(|n| box_iou(&n.bbox, &t.bbox) > SAME_PERSON_IOU);
            if !rematched && now.duration_since(t.last_seen) <= TRACK_KEEP {
                next.push(t.clone());
            }
        }
        self.tracks = next;
        hidden
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: f32 = 480.0;
    const ME: [f32; 4] = [200.0, 80.0, 240.0, 400.0];
    const MY_FACE: [f32; 4] = [270.0, 120.0, 100.0, 120.0];

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn my_silhouette_stays_me_when_i_turn_away() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        assert!(p.update(&[ME], &[(MY_FACE, true)], H, 0.3, t0).is_empty());
        // Visage tourné : plus de visage, la silhouette bouge un peu.
        let moved = [210.0, 85.0, 235.0, 395.0];
        let mut t = 0;
        for _ in 0..20 {
            t += 1500;
            assert!(
                p.update(&[moved], &[], H, 0.3, at(t0, t)).is_empty(),
                "toujours moi après {t} ms"
            );
        }
    }

    #[test]
    fn hidden_stranger_is_confirmed_after_a_delay() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        assert!(p.update(&[ME], &[], H, 0.3, t0).is_empty(), "pas tout de suite");
        assert!(
            p.update(&[ME], &[], H, 0.3, at(t0, 1500)).is_empty(),
            "toujours en attente"
        );
        assert_eq!(p.update(&[ME], &[], H, 0.3, at(t0, 3000)), vec![ME], "confirmée");
    }

    #[test]
    fn face_seen_during_confirmation_cancels_it() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        p.update(&[ME], &[], H, 0.3, t0);
        assert!(p.update(&[ME], &[(MY_FACE, true)], H, 0.3, at(t0, 1500)).is_empty());
        assert!(
            p.update(&[ME], &[], H, 0.3, at(t0, 6000)).is_empty(),
            "reconnu entre-temps : c'est moi"
        );
    }

    #[test]
    fn visible_unknown_face_is_left_to_face_recognition() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        for t in [0, 1500, 3000, 4500] {
            assert!(p.update(&[ME], &[(MY_FACE, false)], H, 0.3, at(t0, t)).is_empty());
        }
    }

    #[test]
    fn far_away_people_are_ignored() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        let far = [500.0, 200.0, 40.0, 90.0]; // 19 % de la hauteur
        for t in [0, 1500, 3000, 4500] {
            assert!(p.update(&[far], &[], H, 0.3, at(t0, t)).is_empty());
        }
        // Avec une sensibilité « large » (15 %), elle compte.
        let mut p = Presence::default();
        p.update(&[far], &[], H, 0.15, t0);
        assert_eq!(p.update(&[far], &[], H, 0.15, at(t0, 3000)), vec![far]);
    }

    #[test]
    fn stranger_behind_me_is_detected() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        let behind = [20.0, 60.0, 170.0, 380.0];
        p.update(&[ME, behind], &[(MY_FACE, true)], H, 0.3, t0);
        assert!(p
            .update(&[ME, behind], &[(MY_FACE, true)], H, 0.3, at(t0, 1500))
            .is_empty());
        assert_eq!(
            p.update(&[ME, behind], &[(MY_FACE, true)], H, 0.3, at(t0, 3000)),
            vec![behind]
        );
    }

    #[test]
    fn someone_taking_my_seat_after_i_left_is_a_stranger() {
        let (mut p, t0) = (Presence::default(), Instant::now());
        p.update(&[ME], &[(MY_FACE, true)], H, 0.3, t0);
        // Je pars : plus personne pendant 10 s, ma silhouette est oubliée.
        p.update(&[], &[], H, 0.3, at(t0, 10_000));
        p.update(&[ME], &[], H, 0.3, at(t0, 11_000));
        assert_eq!(p.update(&[ME], &[], H, 0.3, at(t0, 14_000)), vec![ME]);
    }
}
