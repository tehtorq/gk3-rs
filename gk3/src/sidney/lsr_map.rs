//! SIDNEY's map of the Rennes-le-Chateau valley: Le Serpent Rouge's
//! puzzle. Each sign of the zodiac is solved by marking points on the map,
//! or by placing, sizing and turning a shape (circle, square, hexagram)
//! from the saved shapes, then asking SIDNEY to analyse it.
//!
//! As G-Engine's `SidneyAnalyze_Map`, `MapState` and `MapView`
//! (GPL-3.0), with their numbers. Two maps show at once: a close view of
//! the big map (1368 pixels square) that can be panned, and the whole of
//! the little one (342). Points and lines are kept in the big map's
//! units, shapes and grids in the little one's; both have y going up from
//! the bottom left, as G-Engine's.
//!
//! What has been solved is drawn from the story's flags, so a saved game
//! needs nothing more; work not yet locked in lasts while the game runs.

use gk3_barn::Library;
use gk3_game::state::GameState;
use gk3_render::{Overlay, OverlayTexture, Renderer};
use glam::Vec2;

use super::steps::{Popup, Step, ToolStep};
use super::{Event, Hit, Sidney};
use crate::ui::{Align, Space, Ui};

/// The maps' sizes.
pub const BIG: f32 = 1368.0;
pub const LITTLE: f32 = 342.0;
/// Where the close view and the little map are, in SIDNEY's pixels.
pub const VIEW: [f32; 4] = [9.0, 83.0, 271.0, 324.0];
pub const WHOLE: [f32; 4] = [289.0, 83.0, LITTLE, LITTLE];

pub fn to_little(p: Vec2) -> Vec2 {
    p / BIG * LITTLE
}

pub fn to_big(p: Vec2) -> Vec2 {
    p / LITTLE * BIG
}

// Where things are, on the big map.
const RLC: Vec2 = Vec2::new(267.0, 953.0);
const CDB: Vec2 = Vec2::new(652.0, 1061.0);
const SUNRISE_END: Vec2 = Vec2::new(1336.0, 1247.0);
const COUSTAUSSA: Vec2 = Vec2::new(404.0, 1095.0);
const BEZU: Vec2 = Vec2::new(301.0, 386.0);
const BUGARACH: Vec2 = Vec2::new(990.0, 326.0);
const SERRES: Vec2 = Vec2::new(808.0, 1168.0);
const MERIDIAN: Vec2 = Vec2::new(896.0, 1130.0);
const LERMITAGE: Vec2 = Vec2::new(676.0, 696.0);
const POUSSIN_TOMB: Vec2 = Vec2::new(936.0, 1208.0);
const TEMPLE_CORNERS: [Vec2; 4] = [
    Vec2::new(381.0, 292.0),
    Vec2::new(605.0, 201.0),
    Vec2::new(970.0, 1102.0),
    Vec2::new(745.0, 1193.0),
];
const TEMPLE_DIVISIONS: [Vec2; 4] = [
    Vec2::new(654.0, 967.0),
    Vec2::new(879.0, 877.0),
    Vec2::new(471.0, 518.0),
    Vec2::new(695.0, 426.0),
];
const THE_SITE: Vec2 = Vec2::new(812.0, 1033.0);
const SERPENT_TAIL: Vec2 = Vec2::new(875.0, 1163.0);
const SERPENT_HEAD: Vec2 = Vec2::new(735.0, 1365.0);
const SERPENT_MORE: [Vec2; 4] = [
    Vec2::new(875.0, 1206.0),
    Vec2::new(837.0, 1238.0),
    Vec2::new(801.0, 1267.0),
    Vec2::new(765.0, 1301.0),
];

// The circle, and the square and hexagram on it, on the little map.
const CIRCLE_CENTER: Vec2 = Vec2::new(169.0, 174.0);
const CIRCLE_RADIUS: f32 = 121.0;
const SQUARE_SIZE: f32 = CIRCLE_RADIUS * 2.0 + 1.0;
const SQUARE_TURN: f32 = 1.185174;
const HEXAGRAM_DEGREES: f32 = 33.0;

/// How near a placed point must be to count, on the big map; and how near
/// the pointer must be to a point to name it, on the little one.
const NEAR: f32 = 20.0;
const HOVER: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Circle,
    Square,
    Hexagram,
}

/// A shape on the little map: `size` is a radius, or a square's side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    pub center: Vec2,
    pub size: f32,
    pub angle: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub center: Vec2,
    pub size: f32,
    pub angle: f32,
    pub divisions: u32,
    pub border: bool,
}

/// What a press on the little map commits to until let go.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Drag {
    #[default]
    None,
    Select,
    Move,
    Resize,
    Rotate,
    Focus,
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    Vec2::from_angle(angle).rotate(v)
}

/// A hexagram's six corners about its centre, not yet turned.
fn hexagram_corners(radius: f32) -> [Vec2; 6] {
    std::array::from_fn(|j| {
        let a = j as f32 * std::f32::consts::TAU / 6.0;
        Vec2::new(radius * a.sin(), radius * a.cos())
    })
}

fn closest_on_segment(a: Vec2, b: Vec2, p: Vec2) -> Vec2 {
    let ab = b - a;
    let t = if ab.length_squared() > 0.0 {
        ((p - a).dot(ab) / ab.length_squared()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    a + ab * t
}

impl Shape {
    /// The pointer is on its outline.
    fn on_edge(&self, p: Vec2) -> bool {
        let local = rotate(p - self.center, -self.angle);
        match self.kind {
            Kind::Circle => ((p - self.center).length() - self.size).abs() < 2.5,
            Kind::Square => {
                let half = self.size * 0.5;
                let (dx, dy) = (local.x.abs(), local.y.abs());
                ((half - dx).abs() < 2.5 && dy <= half) || ((half - dy).abs() < 2.5 && dx <= half)
            }
            Kind::Hexagram => {
                let c = hexagram_corners(self.size);
                [(0, 2), (2, 4), (4, 0), (1, 3), (3, 5), (5, 1)]
                    .iter()
                    .any(|&(a, b)| {
                        (closest_on_segment(c[a], c[b], local) - local).length_squared() < 16.0
                    })
            }
        }
    }

    fn inside(&self, p: Vec2) -> bool {
        match self.kind {
            Kind::Square => {
                let local = rotate(p - self.center, -self.angle);
                let half = self.size * 0.5;
                local.x.abs() < half && local.y.abs() < half
            }
            _ => (p - self.center).length_squared() < self.size * self.size,
        }
    }

    /// Its outline as line segments, scaled by `scale`.
    fn segments(&self, scale: f32) -> Vec<(Vec2, Vec2)> {
        let c = self.center * scale;
        let r = self.size * scale;
        match self.kind {
            Kind::Circle => {
                let n = 62;
                let at = |j: usize| {
                    let a = j as f32 * 0.1;
                    c + Vec2::new(r * a.sin(), r * a.cos())
                };
                (0..n)
                    .map(|j| (at(j), if j + 1 < n { at(j + 1) } else { at(0) }))
                    .collect()
            }
            Kind::Square => {
                let h = r * 0.5;
                let k = [(-h, h), (h, h), (h, -h), (-h, -h)]
                    .map(|(x, y)| c + rotate(Vec2::new(x, y), self.angle));
                (0..4).map(|i| (k[i], k[(i + 1) % 4])).collect()
            }
            Kind::Hexagram => {
                let k = hexagram_corners(r).map(|v| c + rotate(v, self.angle));
                [(0, 2), (2, 4), (4, 0), (1, 3), (3, 5), (5, 1)]
                    .iter()
                    .map(|&(a, b)| (k[a], k[b]))
                    .collect()
            }
        }
    }
}

impl Grid {
    fn segments(&self, scale: f32) -> Vec<(Vec2, Vec2)> {
        let c = self.center * scale;
        let h = self.size * scale * 0.5;
        let corner = |x: f32, y: f32| c + rotate(Vec2::new(x, y), self.angle);
        let (tl, tr, bl, br) = (corner(-h, h), corner(h, h), corner(-h, -h), corner(h, -h));
        let mut out = Vec::new();
        if self.border {
            out.extend([(tl, bl), (bl, br), (br, tr), (tr, tl)]);
        }
        let n = self.divisions.max(1) as f32;
        for i in 1..self.divisions {
            let t = i as f32 / n;
            out.push((tl.lerp(tr, t), bl.lerp(br, t)));
            out.push((tl.lerp(bl, t), tr.lerp(br, t)));
        }
        out
    }
}

/// The map's state.
#[derive(Debug, Clone, Default)]
pub struct Map {
    /// On the big map.
    pub points: Vec<Vec2>,
    pub locked_points: Vec<Vec2>,
    pub lines: Vec<(Vec2, Vec2)>,
    /// On the little map.
    pub shapes: Vec<Shape>,
    pub locked: Vec<Shape>,
    pub grid: Option<Grid>,
    pub locked_grid: Option<Grid>,
    pub selected: Option<usize>,
    pub drag: Drag,
    drag_from: Vec2,
    drag_center: Vec2,
    pub entering: bool,
    /// The close view's bottom left, on the big map.
    pub pan: Vec2,
    /// The note at the foot, and until when (forever if `None`).
    pub status: Option<(String, Option<f64>)>,
}

/// The little map's square, as Aries leaves it (not yet turned).
fn sized_square() -> Shape {
    Shape {
        kind: Kind::Square,
        center: CIRCLE_CENTER,
        size: SQUARE_SIZE,
        angle: 0.0,
    }
}

fn locked_circle() -> Shape {
    Shape {
        kind: Kind::Circle,
        center: CIRCLE_CENTER,
        size: CIRCLE_RADIUS,
        angle: 0.0,
    }
}

fn locked_square() -> Shape {
    Shape {
        angle: SQUARE_TURN,
        ..sized_square()
    }
}

fn locked_hexagram() -> Shape {
    Shape {
        kind: Kind::Hexagram,
        center: CIRCLE_CENTER,
        size: CIRCLE_RADIUS,
        angle: HEXAGRAM_DEGREES.to_radians(),
    }
}

fn chessboard() -> Grid {
    let s = locked_square();
    Grid {
        center: s.center,
        size: s.size,
        angle: s.angle,
        divisions: 8,
        border: false,
    }
}

fn has(state: &GameState, flag: &str) -> bool {
    state.flags.contains(&gk3_game::state::key(flag))
}

impl Map {
    /// The map with what the story has solved on it.
    pub fn from_flags(state: &GameState) -> Self {
        let f = |name: &str| has(state, name);
        let mut m = Self::default();
        if f("Aquarius") {
            m.lock(&[RLC, CDB]);
            m.lines.push((RLC, SUNRISE_END));
        }
        if f("Pisces") {
            m.lock(&[COUSTAUSSA, BEZU, BUGARACH]);
            m.locked.push(locked_circle());
        }
        if f("PlacedMeridianLine") {
            m.lock(&[SERRES, MERIDIAN]);
            m.lines.push((SERRES, MERIDIAN));
        }
        if f("Taurus") {
            m.locked.push(locked_square());
        } else if f("Aries") {
            // Sized, but still to be turned.
            m.shapes.push(sized_square());
        }
        if f("Gemini") || f("PlacedGrid") {
            m.locked_grid = Some(chessboard());
        }
        if f("Leo") {
            m.lock(&[LERMITAGE, POUSSIN_TOMB]);
            m.lines.push((LERMITAGE, POUSSIN_TOMB));
        }
        if f("Virgo") {
            m.lock(&TEMPLE_CORNERS);
            for i in 0..4 {
                m.lines
                    .push((TEMPLE_CORNERS[i], TEMPLE_CORNERS[(i + 1) % 4]));
            }
        }
        if f("Libra") {
            m.locked.push(locked_hexagram());
        }
        if f("PlacedTempleDivisions") {
            m.lock(&TEMPLE_DIVISIONS);
            m.lines.push((TEMPLE_DIVISIONS[0], TEMPLE_DIVISIONS[1]));
            m.lines.push((TEMPLE_DIVISIONS[2], TEMPLE_DIVISIONS[3]));
        }
        if f("MarkedTheSite") {
            m.lock(&[THE_SITE]);
        }
        if f("Sagittarius") {
            m.lock(&[SERPENT_TAIL, SERPENT_HEAD]);
        }
        m
    }

    fn lock(&mut self, points: &[Vec2]) {
        self.locked_points.extend_from_slice(points);
    }

    /// A placed point near one wanted (or a locked one, if `locked`).
    pub fn near(&self, wanted: Vec2, locked: bool) -> Option<Vec2> {
        let points = if locked {
            &self.locked_points
        } else {
            &self.points
        };
        points
            .iter()
            .copied()
            .find(|p| (*p - wanted).length_squared() < NEAR * NEAR)
    }

    fn remove(&mut self, p: Vec2) {
        if let Some(i) = self.points.iter().position(|q| *q == p) {
            self.points.remove(i);
        }
    }

    /// Placed points near all of these, if there are.
    fn all_near(&self, wanted: &[Vec2]) -> Option<Vec<Vec2>> {
        wanted.iter().map(|w| self.near(*w, false)).collect()
    }

    /// Takes the placed points away and locks the right ones in.
    fn replace(&mut self, placed: &[Vec2], right: &[Vec2]) {
        for p in placed {
            self.remove(*p);
        }
        self.lock(right);
    }

    fn shape(&self, kind: Kind) -> Option<usize> {
        self.shapes.iter().position(|s| s.kind == kind)
    }

    pub fn clear_selection(&mut self) {
        self.selected = None;
    }

    /// The close view centred on a place on the big map, kept on the map.
    pub fn focus(&mut self, at: Vec2) {
        let half = Vec2::new(VIEW[2], VIEW[3]) * 0.5;
        let max = Vec2::new(BIG - VIEW[2], BIG - VIEW[3]);
        self.pan = (at - half).clamp(Vec2::ZERO, max);
    }

    /// What pressing at `p` (on the little map) would do to the shape
    /// selected. Once Aries is solved the square can only be turned.
    fn would(&self, p: Vec2, aries: bool) -> Drag {
        let Some(s) = self.selected.and_then(|i| self.shapes.get(i)) else {
            return Drag::None;
        };
        match s.kind {
            Kind::Circle if s.on_edge(p) => Drag::Resize,
            Kind::Circle if s.inside(p) => Drag::Move,
            Kind::Circle => Drag::None,
            Kind::Square if s.on_edge(p) => {
                if aries {
                    Drag::None
                } else {
                    Drag::Resize
                }
            }
            Kind::Square if s.inside(p) => {
                if aries {
                    Drag::None
                } else {
                    Drag::Move
                }
            }
            _ if s.on_edge(p) => Drag::Resize,
            _ if s.inside(p) => Drag::Move,
            _ => Drag::Rotate,
        }
    }

    /// The pointer's look over the little map.
    pub fn cursor(&self, p: Vec2, aries: bool) -> Option<&'static str> {
        let doing = match self.drag {
            Drag::Move | Drag::Resize | Drag::Rotate => self.drag,
            _ => self.would(p, aries),
        };
        match doing {
            Drag::Resize => Some("C_DRAGRESIZED1"),
            Drag::Move => Some("C_DRAGMOVE"),
            Drag::Rotate => Some("C_TURNRIGHT"),
            _ => None,
        }
    }

    /// A press on the little map: picks a shape by its outline, or begins
    /// moving, sizing or turning the one picked, or moves the close view.
    pub fn press(&mut self, p: Vec2, aries: bool) {
        for (i, s) in self.shapes.iter().enumerate() {
            if s.on_edge(p) && self.selected != Some(i) {
                self.selected = Some(i);
                self.drag = Drag::Select;
                break;
            }
        }
        if self.drag == Drag::None {
            self.drag = match self.would(p, aries) {
                Drag::None => Drag::Focus,
                d => d,
            };
        }
        self.drag_from = p;
        if let Some(s) = self.selected.and_then(|i| self.shapes.get(i)) {
            self.drag_center = s.center;
        }
        self.held(p);
    }

    /// The pointer moved with the button down.
    pub fn held(&mut self, p: Vec2) {
        if self.drag == Drag::Focus {
            self.focus(to_big(p));
            return;
        }
        let Some(s) = self.selected.and_then(|i| self.shapes.get_mut(i)) else {
            return;
        };
        match self.drag {
            Drag::Move => s.center = p + (self.drag_center - self.drag_from),
            Drag::Resize if s.kind == Kind::Circle => s.size = (s.center - p).length(),
            Drag::Resize => {
                let moved = p - self.drag_from;
                let out = p - s.center;
                let change = moved.length() * if moved.dot(out) < 0.0 { -1.0 } else { 1.0 };
                self.drag_from = p;
                s.size = (s.size + change).max(1.0);
            }
            Drag::Rotate if s.kind != Kind::Circle => {
                let before = (self.drag_from - self.drag_center).normalize_or_zero();
                let now = (p - self.drag_center).normalize_or_zero();
                self.drag_from = p;
                let mut turn = before.dot(now).clamp(-1.0, 1.0).acos();
                if before.perp_dot(now) < 0.0 {
                    turn = -turn;
                }
                s.angle += turn;
            }
            _ => {}
        }
    }

    pub fn release(&mut self) {
        self.drag = Drag::None;
    }

    /// A placed point under the pointer (on the little map), as the big
    /// map has it.
    pub fn point_at(&self, p: Vec2) -> Option<Vec2> {
        self.points
            .iter()
            .chain(&self.locked_points)
            .copied()
            .find(|q| (to_little(*q) - p).length_squared() < HOVER * HOVER)
    }

    /// Draws a grid over the whole map, or in the locked square.
    pub fn draw_grid(&mut self, divisions: u32, in_shape: bool) {
        self.grid = if in_shape {
            self.locked
                .iter()
                .find(|s| s.kind == Kind::Square)
                .map(|s| Grid {
                    center: s.center,
                    size: s.size,
                    angle: s.angle,
                    divisions,
                    border: false,
                })
        } else {
            Some(Grid {
                center: Vec2::splat(LITTLE / 2.0),
                size: LITTLE,
                angle: 0.0,
                divisions,
                border: true,
            })
        };
    }

    pub fn erase_selected(&mut self) {
        if let Some(i) = self.selected.take()
            && i < self.shapes.len()
        {
            self.shapes.remove(i);
        }
    }

    /// Point on the big map as SIDNEY gives it: longitude and latitude.
    pub fn lat_long(format: &str, p: Vec2) -> String {
        let n = p / (BIG - 1.0);
        let long = (n.x * 521.0).round() as i32;
        let lat = (n.y * 281.0).round() as i32;
        let (long_deg, long_min) = if long < 30 {
            (14, long + 30)
        } else {
            (15 + (long - 30) / 60, (long - 30) % 60)
        };
        let (lat_deg, lat_min) = if lat < 27 {
            (52, lat + 33)
        } else {
            (53 + (lat - 27) / 60, (lat - 27) % 60)
        };
        printf(format, &[2, long_deg, long_min, 42, lat_deg, lat_min], "")
    }
}

/// Fills a C format's `%d`s with numbers and `%s` with text.
fn printf(format: &str, numbers: &[i32], text: &str) -> String {
    let mut out = String::new();
    let mut n = numbers.iter();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('d') => out.push_str(&n.next().map(|v| v.to_string()).unwrap_or_default()),
            Some('s') => out.push_str(text),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

/// A pointer in SIDNEY's pixels on the little map, in its units.
pub fn on_little(at: (f32, f32)) -> Option<Vec2> {
    let inside = at.0 >= WHOLE[0]
        && at.1 >= WHOLE[1]
        && at.0 < WHOLE[0] + WHOLE[2]
        && at.1 < WHOLE[1] + WHOLE[3];
    inside.then(|| Vec2::new(at.0 - WHOLE[0], WHOLE[1] + WHOLE[3] - at.1))
}

/// A pointer on the close view, on the big map.
pub fn on_view(at: (f32, f32), pan: Vec2) -> Option<Vec2> {
    let inside =
        at.0 >= VIEW[0] && at.1 >= VIEW[1] && at.0 < VIEW[0] + VIEW[2] && at.1 < VIEW[1] + VIEW[3];
    inside.then(|| Vec2::new(at.0 - VIEW[0], VIEW[1] + VIEW[3] - at.1) + pan)
}

/// Draws on one of the maps: from map units (y up) to SIDNEY's pixels,
/// cut to the map's box.
struct Pen {
    clip: [f32; 4],
    /// SIDNEY pixel of map (0, 0), and map units per pixel.
    origin: Vec2,
    scale: f32,
}

impl Pen {
    fn to_screen(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.origin.x + p.x * self.scale,
            self.origin.y - p.y * self.scale,
        )
    }

    fn rect(&self, overlay: &mut Overlay, space: Space, r: [f32; 4], color: [u8; 4]) {
        let x0 = r[0].max(self.clip[0]);
        let y0 = r[1].max(self.clip[1]);
        let x1 = (r[0] + r[2]).min(self.clip[0] + self.clip[2]);
        let y1 = (r[1] + r[3]).min(self.clip[1] + self.clip[3]);
        if x1 > x0 && y1 > y0 {
            overlay.rect(space.rect([x0, y0, x1 - x0, y1 - y0]), color);
        }
    }

    /// A line two pixels wide, cut to the box first.
    fn line(&self, overlay: &mut Overlay, space: Space, (a, b): (Vec2, Vec2), color: [u8; 4]) {
        let (a, b) = (self.to_screen(a), self.to_screen(b));
        let lo = Vec2::new(self.clip[0] - 1.0, self.clip[1] - 1.0);
        let hi = Vec2::new(
            self.clip[0] + self.clip[2] + 1.0,
            self.clip[1] + self.clip[3] + 1.0,
        );
        let Some((a, b)) = clip_segment(a, b, lo, hi) else {
            return;
        };
        let d = b - a;
        let n = d.abs().max_element().ceil().max(1.0) as usize;
        for i in 0..=n {
            let p = a + d * (i as f32 / n as f32);
            self.rect(overlay, space, [p.x - 1.0, p.y - 1.0, 2.0, 2.0], color);
        }
    }

    fn point(&self, overlay: &mut Overlay, space: Space, p: Vec2, color: [u8; 4]) {
        let p = self.to_screen(p);
        self.rect(overlay, space, [p.x - 3.0, p.y - 3.0, 6.0, 6.0], color);
    }

    /// A picture whose bottom left is at `at`, cut to the box.
    fn picture(
        &self,
        overlay: &mut Overlay,
        space: Space,
        (tex, w, h): (OverlayTexture, u32, u32),
        at: Vec2,
        tint: [u8; 4],
    ) {
        let (w, h) = (w as f32, h as f32);
        let top_left = self.to_screen(at) - Vec2::new(0.0, h);
        image_clipped(
            overlay,
            space,
            tex,
            [0.0, 0.0, w, h],
            [top_left.x, top_left.y, w, h],
            self.clip,
            tint,
        );
    }
}

/// Liang-Barsky: the part of a segment inside a box.
fn clip_segment(a: Vec2, b: Vec2, lo: Vec2, hi: Vec2) -> Option<(Vec2, Vec2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - lo.x),
        (d.x, hi.x - a.x),
        (-d.y, a.y - lo.y),
        (d.y, hi.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let r = q / p;
            if p < 0.0 {
                t0 = t0.max(r);
            } else {
                t1 = t1.min(r);
            }
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// Part of a picture, cut to a box, both in SIDNEY's pixels.
fn image_clipped(
    overlay: &mut Overlay,
    space: Space,
    tex: OverlayTexture,
    source: [f32; 4],
    dest: [f32; 4],
    clip: [f32; 4],
    tint: [u8; 4],
) {
    let x0 = dest[0].max(clip[0]);
    let y0 = dest[1].max(clip[1]);
    let x1 = (dest[0] + dest[2]).min(clip[0] + clip[2]);
    let y1 = (dest[1] + dest[3]).min(clip[1] + clip[3]);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let sx = source[2] / dest[2];
    let sy = source[3] / dest[3];
    let src = [
        source[0] + (x0 - dest[0]) * sx,
        source[1] + (y0 - dest[1]) * sy,
        (x1 - x0) * sx,
        (y1 - y0) * sy,
    ];
    overlay.image(tex, src, space.rect([x0, y0, x1 - x0, y1 - y0]), tint);
}

// Colours: shapes green, locked ones blue, lines black, the locked grid
// grey; a picked shape black on the little map. Lighter on the close view.
const GREEN: [u8; 3] = [0, 255, 0];
const BLUE: [u8; 3] = [0, 0, 255];
const BLACK: [u8; 3] = [0, 0, 0];
const GREY: [u8; 3] = [128, 128, 128];

fn rgba(c: [u8; 3], close: bool) -> [u8; 4] {
    [c[0], c[1], c[2], if close { 192 } else { 255 }]
}

/// The game's items for the coordinates Grace writes down.
const PAPERS: [&str; 3] = [
    "GRACE_COORDINATE_PAPER_1",
    "GRACE_COORDINATE_PAPER_2",
    "GRACE_COORDINATE_PAPER_3",
];

/// The shape files, by number, and their pictures.
pub const SHAPE_FILES: [(u32, &str); 4] = [
    (37, "TRIANGLE"),
    (38, "CIRCLE"),
    (39, "RECTANGLE"),
    (40, "HEXAGRAM"),
];

impl Sidney {
    fn map(&mut self) -> &mut Map {
        self.analyze.map.get_or_insert_with(Map::default)
    }

    fn map_note(&mut self, text: String, seconds: Option<f64>) {
        let until = seconds.map(|s| self.clock + s);
        self.map().status = Some((text, until));
    }

    fn point_note(&mut self, key: &str, p: Vec2) {
        let s = &self.strings;
        let where_ = Map::lat_long(&s.get("Analyze Screen", "MapLatLongText"), p);
        let text = printf(&s.get("Analyze Screen", key), &[], &where_);
        self.map_note(text, None);
    }

    /// The map file opened: what the story has solved is put on it the
    /// first time.
    pub(super) fn map_open(&mut self, state: &GameState) {
        if self.analyze.map.is_none() {
            self.analyze.map = Some(Map::from_flags(state));
        }
    }

    /// Whether the map takes the pointer now (nothing else is going on).
    fn map_live(&self) -> bool {
        self.screen == super::Screen::Analyze
            && self.analyze.state == super::analyze::State::Map
            && self.popup.is_none()
            && self.queue.is_empty()
            && self.waiting.is_none()
            && self.menu_open.is_none()
            && !self.files_open
            && self.choice.is_none()
    }

    /// A press of the left button, in SIDNEY's pixels. Returns whether the
    /// map took it.
    pub fn map_press(&mut self, at: (f32, f32), state: &GameState) -> Vec<Event> {
        if !self.map_live() {
            return Vec::new();
        }
        let aries = has(state, "Aries");
        if let Some(p) = on_little(at) {
            self.map().press(p, aries);
            return Vec::new();
        }
        let pan = self.map().pan;
        let Some(p) = on_view(at, pan).filter(|_| self.map().entering) else {
            return Vec::new();
        };
        self.map().points.push(p);
        self.point_note("MapEnterPointNote", p);
        // During Scorpio, marking the site is answered at once.
        if has(state, "Libra") && !has(state, "Scorpio") && has(state, "PlacedTempleDivisions") {
            let steps = self.check_scorpio(p);
            self.then(steps);
        }
        Vec::new()
    }

    pub fn map_held(&mut self, at: (f32, f32)) {
        if self.analyze.state != super::analyze::State::Map {
            return;
        }
        if let Some(m) = &mut self.analyze.map
            && m.drag != Drag::None
        {
            // Held past the edge, the shape follows the pointer anyway.
            let p = Vec2::new(at.0 - WHOLE[0], WHOLE[1] + WHOLE[3] - at.1);
            m.held(p);
        }
    }

    pub fn map_release(&mut self) {
        if let Some(m) = &mut self.analyze.map {
            m.release();
        }
    }

    /// The map is on screen.
    pub fn map_showing(&self) -> bool {
        self.screen == super::Screen::Analyze && self.analyze.state == super::analyze::State::Map
    }

    /// A right click lets go of the shape picked.
    pub fn map_deselect(&mut self) {
        if let Some(m) = &mut self.analyze.map {
            m.clear_selection();
        }
    }

    /// The pointer's look over the map.
    pub fn map_cursor(&self, at: (f32, f32), state: &GameState) -> Option<&'static str> {
        if !self.map_live() {
            return None;
        }
        let m = self.analyze.map.as_ref()?;
        let p = on_little(at)?;
        m.cursor(p, has(state, "Aries"))
    }

    /// Each frame: the pointer over a point names it; the note at the
    /// foot fades; and the signs solved by shapes are checked as they move.
    pub(super) fn map_tick(&mut self, state: &GameState) {
        let at = self.pointer;
        if self.analyze.state != super::analyze::State::Map {
            return;
        }
        let clock = self.clock;
        if let Some(m) = &mut self.analyze.map
            && m.status
                .as_ref()
                .is_some_and(|(_, u)| u.is_some_and(|u| clock >= u))
        {
            m.status = None;
        }
        if !self.map_live() {
            return;
        }
        if let Some(p) = on_little(at)
            && self.map().selected.is_none()
            && let Some(q) = self.map().point_at(p)
        {
            self.point_note("MapHoverPointNote", q);
        }
        let f = |name: &str| has(state, name);
        let steps = if f("Aquarius") && !f("Pisces") {
            self.check_pisces()
        } else if f("Pisces") && !f("Aries") {
            self.check_aries()
        } else if f("Aries") && !f("Taurus") {
            self.check_taurus()
        } else if f("Leo") && f("Virgo") && !f("Libra") {
            self.check_libra()
        } else {
            Vec::new()
        };
        self.then(steps);
    }

    fn analyze_msg(&self, key: &str, centered: bool) -> Step {
        let mut p = Popup::text(self.strings.get("Analyze Screen", key))
            .left()
            .ok(self.strings.get("Analyze Screen", "OKButton"));
        p.center = centered;
        Step::Show(p)
    }

    /// START ANALYSIS on the map: what is checked depends on the sign
    /// being worked on.
    pub(super) fn map_analyze(&mut self, state: &GameState) -> Vec<Step> {
        let f = |name: &str| has(state, name);
        let mut steps = Vec::new();
        let mut valid = false;
        if !f("Aquarius") {
            steps = self.check_aquarius();
        } else if !f("Pisces") {
            if self.map().all_near(&[COUSTAUSSA, BEZU, BUGARACH]).is_some() {
                steps.push(self.analyze_msg("MapSeveralPossNote", false));
            }
        } else if !f("Taurus") {
            steps = self.check_meridian();
        } else if f("Gemini") && f("Cancer") && (!f("Leo") || !f("Virgo")) {
            // Either may come first; if both are marked, Leo goes first.
            let mut leo = false;
            if !f("Leo") {
                steps = self.check_leo();
                leo = steps
                    .iter()
                    .any(|s| matches!(s, Step::Do(Event::SetFlag("Leo"))));
            }
            if !f("Virgo") && !leo {
                steps.extend(self.check_virgo());
            }
        } else if f("Libra") && !f("Scorpio") {
            let divisions = self.check_temple_divisions(state);
            valid = !divisions.is_empty();
            steps = divisions;
        }
        // Sagittarius can be tried at any time; too early, Grace says so.
        if !valid && !f("Sagittarius") {
            let serpent = self.check_sagittarius(state);
            valid = serpent
                .iter()
                .any(|s| matches!(s, Step::Do(Event::SetFlag("Sagittarius"))));
            steps.extend(serpent);
        }
        valid |= steps.iter().any(|s| matches!(s, Step::Show(_)));
        if !valid {
            let m = self.map();
            let any = !m.points.is_empty() || !m.locked_points.is_empty();
            steps.push(if any {
                self.analyze_msg("MapIndeterminateNote", true)
            } else {
                self.analyze_msg("MapNoPrimitiveNote", false)
            });
        }
        let m = self.map();
        m.entering = false;
        m.clear_selection();
        steps
    }

    fn solved(&self, flags: &[&'static str], score: &str) -> Vec<Step> {
        let mut steps: Vec<Step> = flags.iter().map(|f| Step::Do(Event::SetFlag(f))).collect();
        steps.push(Step::Do(Event::Score(score.into())));
        steps.push(Step::Do(Event::LsrState));
        steps
    }

    fn check_aquarius(&mut self) -> Vec<Step> {
        let Some(placed) = self.map().all_near(&[RLC, CDB]) else {
            return Vec::new();
        };
        let m = self.map();
        m.replace(&placed, &[RLC, CDB]);
        m.lines.push((RLC, SUNRISE_END));
        let mut steps = vec![
            Step::Do(Event::Say("02O3H2Z7F3".into(), 1)),
            self.analyze_msg("MapLine1Note", false),
        ];
        steps.extend(self.solved(&["Aquarius", "PlacedSunriseLine"], "e_sidney_map_aquarius"));
        steps
    }

    fn check_pisces(&mut self) -> Vec<Step> {
        let Some(placed) = self.map().all_near(&[COUSTAUSSA, BEZU, BUGARACH]) else {
            return Vec::new();
        };
        let fits = self.map().shapes.iter().any(|s| {
            s.kind == Kind::Circle
                && (s.center - CIRCLE_CENTER).length_squared() < 20.0 * 20.0
                && (s.size - CIRCLE_RADIUS).abs() < 4.0
        });
        if !fits {
            return Vec::new();
        }
        let m = self.map();
        m.replace(&placed, &[COUSTAUSSA, BEZU, BUGARACH]);
        // A point put on Rennes-le-Chateau again can go.
        if let Some(p) = m.near(RLC, false) {
            m.remove(p);
        }
        m.shapes.retain(|s| s.kind != Kind::Circle);
        m.selected = None;
        m.drag = Drag::None;
        m.locked.push(locked_circle());
        let s = &self.strings;
        let where_ = Map::lat_long(
            &s.get("Analyze Screen", "MapLatLongText"),
            to_big(CIRCLE_CENTER),
        );
        let text = printf(
            &s.get("Analyze Screen", "MapCircleConfirmNote"),
            &[],
            &where_,
        );
        let note = Popup::text(text)
            .left()
            .ok(s.get("Analyze Screen", "OKButton"));
        let mut steps = vec![Step::Show(note), Step::Do(Event::GiveItem(PAPERS[0]))];
        steps.extend(self.solved(&["Pisces", "LockedCircle"], "e_sidney_map_circle"));
        steps.extend([
            Step::Do(Event::Say("02OAG2ZJU2".into(), 2)),
            Step::Talk,
            Step::Do(Event::Sound("CLOCKTIMEBLOCK")),
        ]);
        steps
    }

    fn check_aries(&mut self) -> Vec<Step> {
        let m = self.map();
        let Some(i) = m.shapes.iter().position(|s| {
            s.kind == Kind::Square
                && (s.center - CIRCLE_CENTER).length_squared() < 20.0 * 20.0
                && (s.size - SQUARE_SIZE).abs() < 4.0
        }) else {
            return Vec::new();
        };
        // Put right, still to be turned; the player lets go of it.
        m.drag = Drag::None;
        let angle = m.shapes[i].angle;
        m.shapes[i] = Shape {
            angle,
            ..sized_square()
        };
        let mut steps = vec![Step::Do(Event::Say("02O7E2ZIS1".into(), 1))];
        steps.extend(self.solved(&["Aries", "SizedSquare"], "e_sidney_map_aries"));
        steps
    }

    fn check_meridian(&mut self) -> Vec<Step> {
        let Some(placed) = self.map().all_near(&[SERRES, MERIDIAN]) else {
            return Vec::new();
        };
        let mut steps = vec![self.analyze_msg("MapLine2Note", false)];
        let m = self.map();
        for p in &placed {
            m.remove(*p);
        }
        // Only the first time is new.
        if m.near(SERRES, true).is_none() {
            m.lock(&[SERRES, MERIDIAN]);
            m.lines.push((SERRES, MERIDIAN));
            steps.extend([
                Step::Do(Event::Say("02O3H2ZQB2".into(), 1)),
                Step::Do(Event::SetFlag("PlacedMeridianLine")),
                Step::Do(Event::Score("e_sidney_map_serres".into())),
            ]);
        }
        steps
    }

    fn check_taurus(&mut self) -> Vec<Step> {
        let m = self.map();
        if m.near(SERRES, true).is_none() {
            return Vec::new();
        }
        let Some(i) = m.shape(Kind::Square) else {
            return Vec::new();
        };
        let angle = m.shapes[i].angle.rem_euclid(std::f32::consts::TAU);
        let right = (0..4).any(|k| {
            let want = SQUARE_TURN + k as f32 * std::f32::consts::FRAC_PI_2;
            (angle - want).abs() < 0.1
        });
        if !right {
            return Vec::new();
        }
        m.drag = Drag::None;
        m.selected = None;
        m.shapes.retain(|s| s.kind != Kind::Square);
        m.locked.push(locked_square());
        let mut steps = self.solved(&["Taurus", "LockedSquare"], "e_sidney_map_taurus");
        steps.extend([
            Step::Do(Event::Say("02O7E2ZQB1".into(), 1)),
            Step::Talk,
            Step::Do(Event::Sound("CLOCKTIMEBLOCK")),
        ]);
        steps
    }

    /// Gemini and Cancer: an eight by eight grid in the square.
    fn check_gemini(&mut self, divisions: u32) -> Vec<Step> {
        if divisions != 8 {
            return vec![Step::Do(Event::Say("02OD32ZGW1".into(), 1))];
        }
        let m = self.map();
        if let Some(g) = m.grid.take() {
            m.locked_grid = Some(g);
        }
        let mut steps = vec![Step::Do(Event::Say("02OCL2ZJL1".into(), 1)), Step::Talk];
        steps.extend(self.solved(&["Gemini", "Cancer", "PlacedGrid"], "e_sidney_map_gemini"));
        // The end of 205P: on to the hallway, where the next begins.
        steps.push(Step::Do(Event::ChangeLocation("HAL")));
        steps
    }

    fn check_leo(&mut self) -> Vec<Step> {
        let Some(placed) = self.map().all_near(&[LERMITAGE, POUSSIN_TOMB]) else {
            return Vec::new();
        };
        let mut steps = vec![self.analyze_msg("MapLine3Note", true)];
        let m = self.map();
        for p in &placed {
            m.remove(*p);
        }
        if m.near(LERMITAGE, true).is_none() {
            m.lock(&[LERMITAGE, POUSSIN_TOMB]);
            m.lines.push((LERMITAGE, POUSSIN_TOMB));
            steps.push(Step::Do(Event::Say("02O3H2ZBY2".into(), 1)));
            steps.extend(self.solved(&["Leo", "PlacedTombLine"], "e_sidney_map_poussin"));
        }
        steps
    }

    fn check_virgo(&mut self) -> Vec<Step> {
        let Some(placed) = self.map().all_near(&TEMPLE_CORNERS) else {
            return Vec::new();
        };
        let mut steps = vec![self.analyze_msg("MapRectNote", true)];
        let m = self.map();
        for p in &placed {
            m.remove(*p);
        }
        if m.near(TEMPLE_CORNERS[0], true).is_none() {
            m.lock(&TEMPLE_CORNERS);
            for i in 0..4 {
                m.lines
                    .push((TEMPLE_CORNERS[i], TEMPLE_CORNERS[(i + 1) % 4]));
            }
            steps.extend(self.solved(&["Virgo", "PlacedWalls"], "e_sidney_map_virgo"));
            steps.extend([
                Step::Do(Event::Say("02O3H2ZKI2".into(), 1)),
                Step::Talk,
                // This may end the timeblock.
                Step::Do(Event::ExitIfOver),
            ]);
        }
        steps
    }

    fn check_libra(&mut self) -> Vec<Step> {
        let m = self.map();
        let found = m.shapes.iter().any(|s| {
            if s.kind != Kind::Hexagram
                || (s.center - CIRCLE_CENTER).length_squared() >= 20.0 * 20.0
                || (s.size - CIRCLE_RADIUS).abs() >= 4.0
            {
                return false;
            }
            // Every sixty degrees from 33 looks the same.
            let degrees = (s.angle % std::f32::consts::TAU).abs().to_degrees();
            (0..6).any(|k| (degrees - (HEXAGRAM_DEGREES + 60.0 * k as f32)).abs() < 2.0)
        });
        if !found {
            return Vec::new();
        }
        m.drag = Drag::None;
        m.selected = None;
        m.shapes.retain(|s| s.kind != Kind::Hexagram);
        m.locked.push(locked_hexagram());
        let mut steps = self.solved(&["Libra", "LockedHexagram"], "e_sidney_map_libra");
        steps.extend([
            Step::Do(Event::TakeItem(PAPERS[0])),
            Step::Do(Event::GiveItem(PAPERS[1])),
            Step::Do(Event::Say("02O1K2ZC73".into(), 2)),
            Step::Talk,
            Step::Do(Event::ExitIfOver),
        ]);
        steps
    }

    /// Scorpio's first half: the temple's divisions, once its diagram has
    /// been seen.
    fn check_temple_divisions(&mut self, state: &GameState) -> Vec<Step> {
        if !has(state, "OpenedTempleDiagram") || has(state, "PlacedTempleDivisions") {
            return Vec::new();
        }
        let Some(placed) = self.map().all_near(&TEMPLE_DIVISIONS) else {
            return Vec::new();
        };
        let m = self.map();
        m.replace(&placed, &TEMPLE_DIVISIONS);
        m.lines.push((TEMPLE_DIVISIONS[0], TEMPLE_DIVISIONS[1]));
        m.lines.push((TEMPLE_DIVISIONS[2], TEMPLE_DIVISIONS[3]));
        vec![
            Step::Do(Event::Say("02O3H2ZR82".into(), 1)),
            Step::Do(Event::Score("e_sidney_map_temple".into())),
            Step::Do(Event::SetFlag("PlacedTempleDivisions")),
        ]
    }

    /// Scorpio: the site marked. Grace writes it down.
    fn check_scorpio(&mut self, placed: Vec2) -> Vec<Step> {
        if self.map().near(THE_SITE, false).is_none() {
            return Vec::new();
        }
        let m = self.map();
        m.remove(placed);
        m.lock(&[THE_SITE]);
        let s = &self.strings;
        let label = Popup::text(format!(
            "{}\n\n{} {}",
            s.get("Analyze Screen", "SiteTextTitle"),
            s.get("Analyze Screen", "SiteTextPrompt"),
            s.get("Analyze Screen", "SiteText"),
        ))
        .ok(s.get("Analyze Screen", "OKButton"));
        let mut steps = self.solved(&["MarkedTheSite", "Scorpio"], "e_sidney_map_scorpio");
        steps.extend([
            Step::Do(Event::TakeItem(PAPERS[0])),
            Step::Do(Event::TakeItem(PAPERS[1])),
            Step::Do(Event::GiveItem(PAPERS[2])),
            Step::Do(Event::Say("02O8O2ZRA1".into(), 2)),
            Step::Talk,
            Step::Show(label),
        ]);
        steps
    }

    /// Sagittarius: the red serpent's trail, head to tail.
    fn check_sagittarius(&mut self, state: &GameState) -> Vec<Step> {
        if self.map().all_near(&[SERPENT_TAIL, SERPENT_HEAD]).is_none() {
            return Vec::new();
        }
        if !has(state, "Ophiuchus") {
            self.map().points.clear();
            return vec![Step::Do(Event::Say("02O0I27NG1".into(), 1))];
        }
        let m = self.map();
        m.lock(&[SERPENT_TAIL, SERPENT_HEAD]);
        for p in SERPENT_MORE {
            if m.near(p, false).is_some() {
                m.lock(&[p]);
            }
        }
        m.points.clear();
        let mut steps = vec![
            Step::Do(Event::Say("0273H2ZRS2".into(), 1)),
            self.analyze_msg("MapLine4Note", true),
        ];
        steps.extend(self.solved(
            &["PlacedSerpent", "Sagittarius"],
            "e_sidney_map_saggittarius",
        ));
        steps
    }

    /// The Map and Graphic menus' entries, while the map is open.
    pub(super) fn map_item(&mut self, item: super::analyze::Item, state: &GameState) -> Vec<Step> {
        use super::analyze::Item::*;
        let say = |plate: &str| vec![Step::Do(Event::Say(plate.into(), 1))];
        match item {
            EnterPoints => {
                if state.timeblock == "205P"
                    && !(state.has_item(&state.ego, "CHURCH_PAMPHLET")
                        && state.has_item(&state.ego, "LSR"))
                {
                    return say("02O8O2ZPI1");
                }
                let text = self.strings.get("Analyze Screen", "EnterPointsNote");
                self.map_note(text, Some(5.0));
                self.map().entering = true;
                Vec::new()
            }
            ClearPoints => {
                let m = self.map();
                m.entering = false;
                m.points.clear();
                Vec::new()
            }
            DrawGrid => {
                if self.map().grid.is_some() {
                    return vec![self.analyze_msg("GridDispNote", false)];
                }
                let s = &self.strings;
                let get = |k: &str| s.get("Analyze Screen", k);
                self.choice = Some(super::Choice {
                    title: get("GridList"),
                    rows: vec![
                        (
                            super::Row::Text(get("GridFillScreen")),
                            vec![Step::Tool(ToolStep::MapGridFill(false))],
                        ),
                        (
                            super::Row::Text(get("GridFillShape")),
                            vec![Step::Tool(ToolStep::MapGridFill(true))],
                        ),
                        (super::Row::Text(get("GridCancel")), Vec::new()),
                    ],
                });
                Vec::new()
            }
            EraseGrid => {
                if self.map().grid.take().is_none() {
                    return vec![self.analyze_msg("NoGridEraseNote", false)];
                }
                Vec::new()
            }
            UseShape => {
                let rows = SHAPE_FILES
                    .iter()
                    .filter(|(id, _)| state.sidney_files.contains(id))
                    .map(|(_, name)| {
                        (
                            super::Row::Picture(name.to_string()),
                            vec![Step::Tool(ToolStep::MapShape(name))],
                        )
                    })
                    .collect();
                self.choice = Some(super::Choice {
                    title: self.strings.get("Analyze Screen", "ShapeList"),
                    rows,
                });
                Vec::new()
            }
            EraseShape => {
                let m = self.map();
                let Some(i) = m.selected else {
                    return vec![self.analyze_msg("NoShapeNote", true)];
                };
                // Between Aries and Taurus the square is right but for its
                // turn; Grace will not throw it away.
                let square = m.shapes.get(i).is_some_and(|s| s.kind == Kind::Square);
                if square && has(state, "Aries") && !has(state, "Taurus") {
                    return say("02O0I27LN1");
                }
                m.erase_selected();
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Whether the grid is to fill the screen or a shape; then its size.
    pub(super) fn map_grid_fill(&mut self, in_shape: bool, state: &GameState) -> Vec<Step> {
        let gemini = has(state, "Taurus") && !has(state, "Gemini");
        if in_shape && !gemini {
            return vec![Step::Do(Event::Say("02OD32ZNF1".into(), 1))];
        }
        let s = &self.strings;
        let mut rows: Vec<(super::Row, Vec<Step>)> = [2, 4, 8, 12, 16]
            .iter()
            .map(|&n| {
                (
                    super::Row::Text(s.get("Analyze Screen", &format!("Grid{n}"))),
                    vec![Step::Tool(ToolStep::MapGrid(n, in_shape))],
                )
            })
            .collect();
        rows.push((
            super::Row::Text(s.get("Analyze Screen", "GridCancel")),
            Vec::new(),
        ));
        self.choice = Some(super::Choice {
            title: s.get("Analyze Screen", "GridList"),
            rows,
        });
        Vec::new()
    }

    /// A grid drawn. Only in the square, during Gemini, does it help.
    pub(super) fn map_grid(
        &mut self,
        divisions: u32,
        in_shape: bool,
        state: &GameState,
    ) -> Vec<Step> {
        self.map().draw_grid(divisions, in_shape);
        if in_shape && has(state, "Taurus") && !has(state, "Gemini") {
            self.check_gemini(divisions)
        } else {
            vec![Step::Do(Event::Say("02O0I27GT1".into(), 1))]
        }
    }

    /// A shape put on the map, if it is the time for it.
    pub(super) fn map_shape(&mut self, name: &str, state: &GameState) -> Vec<Step> {
        let f = |n: &str| has(state, n);
        let (too_soon, done, kind, size) = match name {
            "CIRCLE" => (!f("Aquarius"), f("Pisces"), Kind::Circle, 200.0),
            "RECTANGLE" => (!f("Pisces"), f("Aries"), Kind::Square, 400.0),
            "HEXAGRAM" => (!(f("Leo") && f("Virgo")), f("Libra"), Kind::Hexagram, 400.0),
            _ => (true, false, Kind::Circle, 0.0),
        };
        if too_soon {
            return vec![Step::Do(Event::Say("02O0I27NG1".into(), 1))];
        }
        if done {
            return vec![Step::Do(Event::Say("02O0I27731".into(), 1))];
        }
        // The size given is on the big map.
        self.map().shapes.push(Shape {
            kind,
            center: to_little(Vec2::new(599.0, 770.0)),
            size: size / BIG * LITTLE,
            angle: 0.0,
        });
        Vec::new()
    }

    /// Draws the two maps, what is on them, and the note at the foot.
    pub(super) fn draw_map(
        &self,
        (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
        overlay: &mut Overlay,
        space: Space,
        state: &GameState,
    ) {
        let Some(m) = &self.analyze.map else {
            return;
        };
        let f = |n: &str| has(state, n);
        // The close view: part of the big map.
        if let Some((tex, _, h)) = ui.bitmap(lib, renderer, "SIDNEYBIGMAP") {
            let src = [m.pan.x, h as f32 - m.pan.y - VIEW[3], VIEW[2], VIEW[3]];
            overlay.image(tex, src, space.rect(VIEW), [255; 4]);
        }
        if let Some((tex, w, h)) = ui.bitmap(lib, renderer, "SIDNEYLITTLEMAP") {
            let (w, h) = (w as f32, h as f32);
            overlay.image(
                tex,
                [0.0, 0.0, w, h],
                space.rect([WHOLE[0], WHOLE[1], w, h]),
                [255; 4],
            );
        }
        let close = Pen {
            clip: VIEW,
            origin: Vec2::new(VIEW[0] - m.pan.x, VIEW[1] + VIEW[3] + m.pan.y),
            scale: 1.0,
        };
        let whole = Pen {
            clip: WHOLE,
            origin: Vec2::new(WHOLE[0], WHOLE[1] + WHOLE[3]),
            scale: 1.0,
        };
        // The site's name and the serpent, under the shapes.
        let mut picture = |pen: &Pen, name: &str, at: Vec2, tint: [u8; 4]| {
            if let Some(b) = ui.bitmap(lib, renderer, name) {
                pen.picture(overlay, space, b, at, tint);
            }
        };
        if f("MarkedTheSite") {
            picture(
                &close,
                "MAPLG_THE",
                Vec2::new(804.0, 1045.0),
                [255, 255, 255, 128],
            );
            picture(
                &close,
                "MAPLG_SITE",
                Vec2::new(777.0, 989.0),
                [255, 255, 255, 128],
            );
            picture(&whole, "MAPSM_THE", Vec2::new(198.0, 257.0), [255; 4]);
            picture(&whole, "MAPSM_SITE", Vec2::new(191.0, 242.0), [255; 4]);
        }
        if f("PlacedSerpent") {
            picture(&close, "SERPENT", Vec2::new(724.0, 1159.0), [255; 4]);
            picture(&whole, "SERPLITMAP", Vec2::new(178.0, 290.0), [255; 4]);
        }
        for (pen, is_close) in [(&close, true), (&whole, false)] {
            // Shapes are kept on the little map.
            let k = if is_close { BIG / LITTLE } else { 1.0 };
            let c = |rgb| rgba(rgb, is_close);
            let mut lines = |segs: Vec<(Vec2, Vec2)>, color: [u8; 4]| {
                for s in segs {
                    pen.line(overlay, space, s, color);
                }
            };
            if let Some(g) = &m.locked_grid {
                lines(g.segments(k), c(GREY));
            }
            if let Some(g) = &m.grid {
                lines(g.segments(k), c(GREEN));
            }
            for kind in [Kind::Square, Kind::Circle, Kind::Hexagram] {
                for s in m.locked.iter().filter(|s| s.kind == kind) {
                    lines(s.segments(k), c(BLUE));
                }
                for (i, s) in m.shapes.iter().enumerate().filter(|(_, s)| s.kind == kind) {
                    let picked = !is_close && m.selected == Some(i);
                    lines(s.segments(k), c(if picked { BLACK } else { GREEN }));
                }
            }
            // Lines and points are kept on the big map.
            let k = if is_close { 1.0 } else { LITTLE / BIG };
            for (a, b) in &m.lines {
                pen.line(overlay, space, (*a * k, *b * k), c(BLACK));
            }
            for p in &m.locked_points {
                pen.point(overlay, space, *p * k, c(BLUE));
            }
            for p in &m.points {
                pen.point(overlay, space, *p * k, c(GREEN));
            }
        }
        if let Some((text, _)) = &m.status {
            ui.write(
                lib,
                renderer,
                overlay,
                space,
                "SID_TEXT_14_GRN",
                text,
                (4.0, 480.0 - 13.0 - 14.0),
                (None, Align::Left),
            );
        }
    }
}

/// A list to choose from, in the file list's box (the grid's sizes, the
/// shapes).
pub(super) fn draw_choice(
    sidney: &Sidney,
    (ui, lib, renderer): (&mut Ui, &Library, &mut Renderer),
    overlay: &mut Overlay,
    space: Space,
    at: (f32, f32),
    hits: &mut Vec<(Hit, [f32; 4])>,
) {
    let Some(choice) = &sidney.choice else {
        return;
    };
    let r = [40.0, 66.0, 154.0, 350.0];
    ui.sid_box(lib, renderer, overlay, space, r, false, [0, 0, 0, 200]);
    ui.write(
        lib,
        renderer,
        overlay,
        space,
        "SID_TEXT_18",
        &choice.title,
        (r[0], r[1] + 2.0),
        (Some(r[2]), Align::Center),
    );
    overlay.rect(
        space.rect([r[0] + 1.0, r[1] + 20.0, r[2] - 2.0, 1.0]),
        [120, 120, 120, 255],
    );
    let close = [r[0] + 2.0, r[1] + 2.0, 16.0, 16.0];
    let up = if super::inside(close, at) {
        "CLOSEWIN_HOVER"
    } else {
        "CLOSEWIN_UP"
    };
    ui.sprite(
        lib,
        renderer,
        overlay,
        space,
        up,
        (close[0], close[1]),
        [255; 4],
    );
    hits.push((Hit::CloseFiles, close));
    let mut y = r[1] + 28.0;
    for (i, (row, _)) in choice.rows.iter().enumerate() {
        match row {
            super::Row::Text(text) => {
                let row = [r[0] + 10.0, y, r[2] - 20.0, 16.0];
                let font = if super::inside(row, at) {
                    "SID_TEXT_14_GRN"
                } else {
                    "SID_TEXT_14"
                };
                ui.write(
                    lib,
                    renderer,
                    overlay,
                    space,
                    font,
                    text,
                    (row[0], y),
                    (None, Align::Left),
                );
                hits.push((Hit::Choice(i), row));
                y += 16.0;
            }
            super::Row::Picture(name) => {
                let (w, h) = ui.bitmap_size(lib, renderer, name).unwrap_or((32.0, 32.0));
                let row = [r[0] + 10.0, y, w, h];
                if let Some(drawn) =
                    ui.sprite(lib, renderer, overlay, space, name, (row[0], y), [255; 4])
                    && super::inside(drawn, at)
                {
                    overlay.rect(space.rect([row[0], y + h - 1.0, w, 1.0]), [0, 255, 0, 255]);
                }
                hits.push((Hit::Choice(i), row));
                y += h;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_scale_into_each_other() {
        let p = Vec2::new(812.0, 1033.0);
        assert!((to_big(to_little(p)) - p).length() < 1e-3);
        assert_eq!(to_little(Vec2::splat(BIG)), Vec2::splat(LITTLE));
    }

    #[test]
    fn coordinates_read_as_the_original() {
        let f = "%d deg %d'%d\" long  %d deg %d'%d\" lat ";
        assert_eq!(
            Map::lat_long(f, Vec2::ZERO),
            "2 deg 14'30\" long  42 deg 52'33\" lat "
        );
        assert_eq!(
            Map::lat_long(f, Vec2::splat(BIG - 1.0)),
            "2 deg 23'11\" long  42 deg 57'14\" lat "
        );
    }

    #[test]
    fn the_close_view_stays_on_the_map() {
        let mut m = Map::default();
        m.focus(Vec2::ZERO);
        assert_eq!(m.pan, Vec2::ZERO);
        m.focus(Vec2::splat(BIG));
        assert_eq!(m.pan, Vec2::new(BIG - VIEW[2], BIG - VIEW[3]));
        m.focus(Vec2::splat(600.0));
        assert_eq!(
            m.pan,
            Vec2::new(600.0 - VIEW[2] / 2.0, 600.0 - VIEW[3] / 2.0)
        );
    }

    #[test]
    fn a_shape_is_picked_by_its_outline_then_moved_sized_and_turned() {
        let mut m = Map::default();
        m.shapes.push(Shape {
            kind: Kind::Square,
            center: Vec2::new(150.0, 190.0),
            size: 100.0,
            angle: 0.0,
        });
        // Its right edge picks it; nothing moves yet.
        m.press(Vec2::new(200.0, 190.0), false);
        assert_eq!((m.selected, m.drag), (Some(0), Drag::Select));
        m.release();
        // Inside, it moves.
        m.press(Vec2::new(150.0, 190.0), false);
        assert_eq!(m.drag, Drag::Move);
        m.held(Vec2::new(169.0, 174.0));
        m.release();
        assert_eq!(m.shapes[0].center, Vec2::new(169.0, 174.0));
        // On the edge, pulled outwards, it grows.
        m.press(Vec2::new(219.0, 174.0), false);
        assert_eq!(m.drag, Drag::Resize);
        m.held(Vec2::new(229.0, 174.0));
        m.release();
        assert_eq!(m.shapes[0].size, 110.0);
        // Outside, it turns: a quarter turn anticlockwise.
        m.press(Vec2::new(269.0, 174.0), false);
        assert_eq!(m.drag, Drag::Rotate);
        m.held(Vec2::new(169.0, 274.0));
        assert!((m.shapes[0].angle - std::f32::consts::FRAC_PI_2).abs() < 1e-4);
        m.release();
        // Once Aries is solved, it can only turn.
        assert_eq!(m.would(Vec2::new(169.0, 174.0), true), Drag::None);
        // Pressing on open map moves the close view there.
        m.clear_selection();
        m.press(Vec2::new(10.0, 10.0), false);
        assert_eq!(m.drag, Drag::Focus);
        assert_eq!(m.pan, Vec2::ZERO);
    }

    #[test]
    fn a_hexagram_is_picked_by_its_lines() {
        let h = locked_hexagram();
        let corner = h.center + rotate(Vec2::new(0.0, h.size), h.angle);
        let other = h.center + rotate(hexagram_corners(h.size)[2], h.angle);
        assert!(h.on_edge(corner.lerp(other, 0.5)));
        assert!(!h.on_edge(h.center));
        assert!(h.inside(h.center));
    }

    #[test]
    fn placed_points_count_when_near() {
        let mut m = Map {
            points: vec![Vec2::new(270.0, 960.0), Vec2::new(640.0, 1050.0)],
            ..Default::default()
        };
        assert!(m.all_near(&[RLC, CDB]).is_some());
        m.points[1] = Vec2::new(600.0, 1050.0);
        assert!(m.all_near(&[RLC, CDB]).is_none());
    }

    #[test]
    fn grids_cut_the_square_into_eight() {
        let g = chessboard();
        assert_eq!(g.segments(1.0).len(), 14, "seven lines each way, no border");
        let mut m = Map::default();
        m.draw_grid(4, false);
        assert_eq!(m.grid.unwrap().segments(1.0).len(), 4 + 6);
        m.draw_grid(8, true);
        assert!(m.grid.is_none(), "no locked square to fill");
    }

    #[test]
    fn segments_are_cut_to_the_box() {
        let lo = Vec2::ZERO;
        let hi = Vec2::splat(10.0);
        let (a, b) = clip_segment(Vec2::new(-10.0, 5.0), Vec2::new(20.0, 5.0), lo, hi).unwrap();
        assert_eq!((a, b), (Vec2::new(0.0, 5.0), Vec2::new(10.0, 5.0)));
        assert!(clip_segment(Vec2::new(-5.0, -5.0), Vec2::new(-1.0, 20.0), lo, hi).is_none());
    }

    #[test]
    fn solved_signs_are_drawn_back() {
        let mut state = GameState::default();
        for f in ["AQUARIUS", "PISCES", "ARIES"] {
            state.flags.insert(f.into());
        }
        let m = Map::from_flags(&state);
        assert_eq!(m.locked_points.len(), 5);
        assert_eq!(m.lines.len(), 1);
        assert_eq!(m.locked, vec![locked_circle()]);
        assert_eq!(m.shapes, vec![sized_square()], "the square, to be turned");
    }
}
