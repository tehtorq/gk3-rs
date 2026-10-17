//! Where people can walk: a room's walk boundary bitmap, and paths across it.
//!
//! The location file names a palettised bitmap and the floor area it covers
//! (`boundary=R25wlkBnds,size={369, 386},offset={39.9, -32.0}`). A texel's
//! palette index is its region: 255 is wall, lower indices are walkable, and
//! regions from 128 up are walkable unless a script blocks them. Mapping
//! follows G-Engine's `WalkerBoundary`.

use std::cmp::Reverse;
use std::collections::{BTreeSet, BinaryHeap, HashMap};
use std::sync::Arc;

use gk3_formats::image::IndexedImage;

#[derive(Debug, Clone)]
pub struct WalkMap {
    width: i32,
    height: i32,
    regions: Arc<Vec<u8>>,
    size: [f32; 2],
    offset: [f32; 2],
    blocked: BTreeSet<u8>,
    /// Floor areas closed off by name (a model standing there), as world
    /// min x, min z, max x, max z: G-Engine's unwalkable rects.
    rects: HashMap<String, [f32; 4]>,
    /// Where other walkers stand, kept clear when finding a way.
    walkers: Vec<[f32; 2]>,
}

/// How near one walker's path may come to another's spot: each is about 10
/// units round, as G-Engine's `WalkerBoundary` reckons.
const WALKER_CLEARANCE: f32 = 20.0;

impl WalkMap {
    pub fn new(image: IndexedImage, size: [f32; 2], offset: [f32; 2]) -> Self {
        Self {
            width: image.width as i32,
            height: image.height as i32,
            regions: Arc::new(image.indices),
            size,
            offset,
            blocked: BTreeSet::from([255]),
            rects: HashMap::new(),
            walkers: Vec::new(),
        }
    }

    /// This map with the floor round other walkers closed off, for finding
    /// one walker's way: nobody's path goes through anybody else, as
    /// G-Engine's walkers are kept apart.
    pub fn avoiding(&self, walkers: &[[f32; 2]]) -> WalkMap {
        WalkMap {
            walkers: walkers.to_vec(),
            ..self.clone()
        }
    }

    /// Closes off a rectangle of floor under a name, or opens it again.
    pub fn set_rect(&mut self, name: &str, rect: Option<[f32; 4]>) {
        let name = name.to_ascii_uppercase();
        match rect {
            Some(r) => self.rects.insert(name, r),
            None => self.rects.remove(&name),
        };
    }

    pub fn set_blocked(&mut self, region: u8, blocked: bool) {
        if blocked {
            self.blocked.insert(region);
        } else if region != 255 {
            self.blocked.remove(&region);
        }
    }

    /// World x, z to texel.
    pub fn texel(&self, x: f32, z: f32) -> (i32, i32) {
        let tx = (x + self.offset[0]) / self.size[0] * self.width as f32;
        let ty = (z + self.offset[1]) / self.size[1] * self.height as f32;
        (tx.floor() as i32, (self.height as f32 - ty).floor() as i32)
    }

    /// A texel's centre in world x, z.
    pub fn world(&self, (tx, ty): (i32, i32)) -> [f32; 2] {
        let x = (tx as f32 + 0.5) / self.width as f32 * self.size[0] - self.offset[0];
        let z = (self.height as f32 - (ty as f32 + 0.5)) / self.height as f32 * self.size[1]
            - self.offset[1];
        [x, z]
    }

    fn region(&self, (x, y): (i32, i32)) -> u8 {
        if x < 0 || y < 0 || x >= self.width || y >= self.height {
            return 255;
        }
        self.regions[(y * self.width + x) as usize]
    }

    /// The region (palette index) of the walk map at a world point.
    pub fn region_at(&self, x: f32, z: f32) -> u8 {
        self.region(self.texel(x, z))
    }

    pub fn walkable_texel(&self, t: (i32, i32)) -> bool {
        if self.blocked.contains(&self.region(t)) {
            return false;
        }
        if self.rects.is_empty() && self.walkers.is_empty() {
            return true;
        }
        let [x, z] = self.world(t);
        let in_rect = self
            .rects
            .values()
            .any(|r| x >= r[0] && x <= r[2] && z >= r[1] && z <= r[3]);
        let near_walker = self.walkers.iter().any(|w| {
            let (dx, dz) = (x - w[0], z - w[1]);
            dx * dx + dz * dz <= WALKER_CLEARANCE * WALKER_CLEARANCE
        });
        !in_rect && !near_walker
    }

    pub fn walkable(&self, x: f32, z: f32) -> bool {
        self.walkable_texel(self.texel(x, z))
    }

    /// The nearest walkable texel, searching outwards.
    fn nearest(&self, from: (i32, i32)) -> Option<(i32, i32)> {
        if self.walkable_texel(from) {
            return Some(from);
        }
        let limit = self.width.max(self.height);
        for r in 1..limit {
            let mut best: Option<((i32, i32), i32)> = None;
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r {
                        continue;
                    }
                    let t = (from.0 + dx, from.1 + dy);
                    let d = dx * dx + dy * dy;
                    if self.walkable_texel(t) && best.is_none_or(|(_, bd)| d < bd) {
                        best = Some((t, d));
                    }
                }
            }
            if let Some((t, _)) = best {
                return Some(t);
            }
        }
        None
    }

    /// The nearest walkable point to a world position.
    pub fn nearest_walkable(&self, x: f32, z: f32) -> Option<[f32; 2]> {
        self.nearest(self.texel(x, z)).map(|t| self.world(t))
    }

    /// Whether a straight line between texels crosses only walkable ground.
    fn clear(&self, a: (i32, i32), b: (i32, i32)) -> bool {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let steps = dx.abs().max(dy.abs()).max(1);
        (0..=steps).all(|i| {
            let t = i as f32 / steps as f32;
            let p = (
                (a.0 as f32 + dx as f32 * t).round() as i32,
                (a.1 as f32 + dy as f32 * t).round() as i32,
            );
            self.walkable_texel(p)
        })
    }

    /// A route from one world point to another, as world x, z waypoints
    /// ending at the walkable point nearest the goal. A* over texels, then
    /// straightened wherever a direct line is clear.
    pub fn path(&self, from: [f32; 2], to: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        self.search(from, to, false)
    }

    /// As `path`, but when the goal cannot be reached, a route to the
    /// reachable point nearest it: G-Engine's "best effort".
    pub fn closest_path(&self, from: [f32; 2], to: [f32; 2]) -> Option<Vec<[f32; 2]>> {
        self.search(from, to, true)
    }

    fn search(&self, from: [f32; 2], to: [f32; 2], best_effort: bool) -> Option<Vec<[f32; 2]>> {
        let start = self.nearest(self.texel(from[0], from[1]))?;
        let goal = self.nearest(self.texel(to[0], to[1]))?;
        if start == goal {
            return Some(vec![self.world(goal)]);
        }
        let index = |(x, y): (i32, i32)| (y * self.width + x) as usize;
        let heuristic = |(x, y): (i32, i32)| {
            let (dx, dy) = ((x - goal.0).abs(), (y - goal.1).abs());
            // Octile distance, in tenths.
            10 * (dx + dy) - 6 * dx.min(dy)
        };
        let mut cost: HashMap<usize, i32> = HashMap::from([(index(start), 0)]);
        let mut came: HashMap<usize, (i32, i32)> = HashMap::new();
        let mut open = BinaryHeap::from([Reverse((heuristic(start), start))]);
        // The cells walked back from `end` to the start, straightened.
        let route = |end: (i32, i32), came: &HashMap<usize, (i32, i32)>| {
            let mut cells = vec![end];
            let mut at = end;
            while let Some(&prev) = came.get(&index(at)) {
                cells.push(prev);
                at = prev;
            }
            cells.reverse();
            self.straighten(&cells)
        };
        let mut closest = start;
        while let Some(Reverse((_, current))) = open.pop() {
            if current == goal {
                return Some(route(goal, &came));
            }
            if heuristic(current) < heuristic(closest) {
                closest = current;
            }
            let here = cost[&index(current)];
            for (dx, dy) in [
                (1, 0),
                (-1, 0),
                (0, 1),
                (0, -1),
                (1, 1),
                (1, -1),
                (-1, 1),
                (-1, -1),
            ] {
                let next = (current.0 + dx, current.1 + dy);
                if !self.walkable_texel(next) {
                    continue;
                }
                let step = if dx != 0 && dy != 0 { 14 } else { 10 };
                let total = here + step;
                if cost.get(&index(next)).is_none_or(|&c| total < c) {
                    cost.insert(index(next), total);
                    came.insert(index(next), current);
                    open.push(Reverse((total + heuristic(next), next)));
                }
            }
        }
        (best_effort && closest != start).then(|| route(closest, &came))
    }

    fn straighten(&self, cells: &[(i32, i32)]) -> Vec<[f32; 2]> {
        let mut out = Vec::new();
        let mut anchor = 0;
        while anchor < cells.len() - 1 {
            let mut far = anchor + 1;
            for j in (anchor + 1..cells.len()).rev() {
                if self.clear(cells[anchor], cells[j]) {
                    far = j;
                    break;
                }
            }
            out.push(self.world(cells[far]));
            anchor = far;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 10x10 texels over 100x100 units from the origin, with a wall down
    /// the middle (x = 5) except at the bottom row.
    fn room() -> WalkMap {
        let mut indices = vec![0u8; 100];
        for y in 0..9 {
            indices[y * 10 + 5] = 255;
        }
        WalkMap::new(
            IndexedImage {
                width: 10,
                height: 10,
                indices,
                palette: Vec::new(),
            },
            [100.0, 100.0],
            [0.0, 0.0],
        )
    }

    #[test]
    fn world_and_texels_agree() {
        let map = room();
        assert_eq!(map.texel(5.0, 95.0), (0, 0), "high z is the top row");
        assert_eq!(map.world((0, 0)), [5.0, 95.0]);
        assert!(!map.walkable(55.0, 50.0));
        assert!(map.walkable(15.0, 50.0));
    }

    #[test]
    fn paths_go_round_walls() {
        let map = room();
        let path = map.path([15.0, 85.0], [85.0, 85.0]).unwrap();
        // Has to dip to the gap at the bottom row (z near 5).
        assert!(path.iter().any(|p| p[1] < 15.0));
        assert!((path.last().unwrap()[0] - 85.0).abs() < 1.0);
        // Straightened: a handful of waypoints, not one per texel.
        assert!(path.len() <= 4, "{path:?}");
    }

    #[test]
    fn paths_go_round_other_walkers() {
        let map = room();
        // Somebody stands on the bottom row's gap through the wall.
        let blocked = map.avoiding(&[[55.0, 5.0]]);
        assert!(!blocked.walkable(55.0, 5.0));
        assert!(!blocked.walkable(70.0, 5.0), "within 20 units of them");
        assert!(blocked.walkable(85.0, 50.0));
        assert!(blocked.path([15.0, 85.0], [85.0, 85.0]).is_none());
        // Elsewhere, the way goes round them.
        let map = room().avoiding(&[[25.0, 50.0]]);
        let path = map.path([25.0, 85.0], [25.0, 15.0]).unwrap();
        let mut at = [25.0f32, 85.0];
        for p in &path {
            for k in 0..=20 {
                let t = k as f32 / 20.0;
                let (x, z) = (at[0] + (p[0] - at[0]) * t, at[1] + (p[1] - at[1]) * t);
                let d = ((x - 25.0).powi(2) + (z - 50.0).powi(2)).sqrt();
                assert!(d > 15.0, "passes {d} from them: {path:?}");
            }
            at = *p;
        }
    }

    /// With the wall closed all the way down, the far side cannot be
    /// reached: no path, but a best effort up to the wall.
    #[test]
    fn cut_off_goals_get_as_near_as_can_be() {
        let mut indices = vec![0u8; 100];
        for y in 0..10 {
            indices[y * 10 + 5] = 255;
        }
        let map = WalkMap::new(
            IndexedImage {
                width: 10,
                height: 10,
                indices,
                palette: Vec::new(),
            },
            [100.0, 100.0],
            [0.0, 0.0],
        );
        assert!(map.path([15.0, 50.0], [85.0, 50.0]).is_none());
        let path = map.closest_path([15.0, 50.0], [85.0, 50.0]).unwrap();
        let end = path.last().unwrap();
        assert!((end[0] - 45.0).abs() < 1.0 && (end[1] - 50.0).abs() < 6.0, "ends by the wall: {end:?}");
    }

    #[test]
    fn unwalkable_goals_snap_to_the_nearest_floor() {
        let map = room();
        let path = map.path([15.0, 50.0], [55.0, 50.0]).unwrap();
        let end = path.last().unwrap();
        assert!(map.walkable(end[0], end[1]));
    }

    #[test]
    fn blocked_rects() {
        let mut map = WalkMap::new(
            IndexedImage {
                width: 10,
                height: 10,
                indices: vec![0; 100],
                palette: Vec::new(),
            },
            [100.0, 100.0],
            [0.0, 0.0],
        );
        assert!(map.walkable(55.0, 55.0));
        map.set_rect("eml", Some([50.0, 50.0, 60.0, 60.0]));
        assert!(!map.walkable(55.0, 55.0));
        assert!(map.walkable(25.0, 25.0));
        map.set_rect("EML", None);
        assert!(map.walkable(55.0, 55.0));
    }

    #[test]
    fn blocked_regions() {
        let mut map = room();
        Arc::make_mut(&mut map.regions)[0] = 130;
        assert!(map.walkable_texel((0, 0)));
        map.set_blocked(130, true);
        assert!(!map.walkable_texel((0, 0)));
    }
}
