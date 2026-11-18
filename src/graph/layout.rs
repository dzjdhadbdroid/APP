//! Placement automatique des nœuds (algorithme de forces), en Rust pur, sans dépendance.

use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Layout {
    pub pos: HashMap<i64, (f32, f32)>,
    vel: HashMap<i64, (f32, f32)>,
}

impl Layout {
    pub fn new() -> Self {
        Self::default()
    }

    /// Aligne l'ensemble des nœuds suivis sur `ids` : ajoute les nouveaux (en spirale,
    /// donc de façon déterministe), retire ceux qui ont disparu.
    pub fn sync(&mut self, ids: &[i64]) {
        let keep: HashSet<i64> = ids.iter().copied().collect();
        self.pos.retain(|id, _| keep.contains(id));
        self.vel.retain(|id, _| keep.contains(id));
        for (i, id) in ids.iter().enumerate() {
            self.pos.entry(*id).or_insert_with(|| {
                let a = i as f32 * 2.4;
                let r = 45.0 * ((i + 1) as f32).sqrt();
                (r * a.cos(), r * a.sin())
            });
            self.vel.entry(*id).or_insert((0.0, 0.0));
        }
    }

    /// Une itération de simulation. `edges` : paires d'identifiants connectés.
    pub fn step(&mut self, edges: &[(i64, i64)]) {
        const REPULSION: f32 = 9000.0;
        const SPRING: f32 = 0.02;
        const REST: f32 = 110.0;
        const GRAVITY: f32 = 0.004;
        const DAMPING: f32 = 0.82;
        const MAX_SPEED: f32 = 25.0;

        let ids: Vec<i64> = self.pos.keys().copied().collect();
        let mut force: HashMap<i64, (f32, f32)> = ids.iter().map(|i| (*i, (0.0, 0.0))).collect();

        for (i, a) in ids.iter().enumerate() {
            for b in &ids[i + 1..] {
                let (pa, pb) = (self.pos[a], self.pos[b]);
                let (dx, dy) = (pa.0 - pb.0, pa.1 - pb.1);
                let d2 = (dx * dx + dy * dy).max(25.0);
                let d = d2.sqrt();
                let f = REPULSION / d2;
                let (fx, fy) = (f * dx / d, f * dy / d);
                let fa = force.get_mut(a).unwrap();
                fa.0 += fx;
                fa.1 += fy;
                let fb = force.get_mut(b).unwrap();
                fb.0 -= fx;
                fb.1 -= fy;
            }
        }
        for (a, b) in edges {
            let (Some(pa), Some(pb)) = (self.pos.get(a).copied(), self.pos.get(b).copied()) else {
                continue;
            };
            let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
            let d = (dx * dx + dy * dy).sqrt().max(1.0);
            let f = SPRING * (d - REST);
            let (fx, fy) = (f * dx / d, f * dy / d);
            let fa = force.get_mut(a).unwrap();
            fa.0 += fx;
            fa.1 += fy;
            let fb = force.get_mut(b).unwrap();
            fb.0 -= fx;
            fb.1 -= fy;
        }
        for id in ids {
            let p = self.pos[&id];
            let f = force[&id];
            let v = self.vel.get_mut(&id).unwrap();
            v.0 = ((v.0 + f.0 - GRAVITY * p.0) * DAMPING).clamp(-MAX_SPEED, MAX_SPEED);
            v.1 = ((v.1 + f.1 - GRAVITY * p.1) * DAMPING).clamp(-MAX_SPEED, MAX_SPEED);
            let p = self.pos.get_mut(&id).unwrap();
            p.0 += v.0;
            p.1 += v.1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dist(l: &Layout, a: i64, b: i64) -> f32 {
        let (pa, pb) = (l.pos[&a], l.pos[&b]);
        ((pa.0 - pb.0).powi(2) + (pa.1 - pb.1).powi(2)).sqrt()
    }

    #[test]
    fn sync_adds_and_removes() {
        let mut l = Layout::new();
        l.sync(&[1, 2, 3]);
        assert_eq!(l.pos.len(), 3);
        l.sync(&[2, 4]);
        assert!(l.pos.contains_key(&2) && l.pos.contains_key(&4) && !l.pos.contains_key(&1));
    }

    #[test]
    fn linked_nodes_end_closer_than_unlinked() {
        let mut l = Layout::new();
        l.sync(&[1, 2, 3, 4]);
        for _ in 0..600 {
            l.step(&[(1, 2), (2, 3)]);
        }
        assert!(dist(&l, 1, 2) < dist(&l, 1, 4));
    }

    #[test]
    fn positions_stay_finite() {
        let mut l = Layout::new();
        let ids: Vec<i64> = (1..=40).collect();
        l.sync(&ids);
        for _ in 0..300 {
            l.step(&[(1, 2), (2, 3), (3, 1)]);
        }
        assert!(l.pos.values().all(|p| p.0.is_finite() && p.1.is_finite()));
    }
}
