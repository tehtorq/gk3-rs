//! The cast in a saved game: each figure as it stands, poses, walks,
//! fidgets and faces included, as the original saves its actors' walkers,
//! fidgets, GAS scripts and animations (`docs/re-saves.md`). What is
//! loaded from the game's files (models, the room's places, the GPU's
//! copies) is not saved: the cast is built again from the room's file as
//! it was on entry, and this is put back over it.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::sync::Arc;

use gk3_barn::Library;
use gk3_formats::act::VertexAnimation;
use gk3_formats::anm::CueKind;
use gk3_game::gas_player::GasPlayer;
use gk3_game::pose::Pose;
use glam::Mat4;
use serde::{Deserialize, Serialize};

use super::{
    ActorPlacement, Cast, FaceState, Fidget, FloorLight, HeadRig, LookTarget, Morph, Playing, Walk,
};
use crate::faces::Expression;
use gk3_formats::anm::FaceChange;

/// Vertex animations are saved by the name they were loaded under, and
/// found again by it. Saving and restoring set up which names and clips
/// are in play for the moment.
pub mod clips {
    use super::*;

    thread_local! {
        /// While saving: each clip's name, by its address; and the names
        /// used.
        static NAMES: RefCell<HashMap<usize, String>> = RefCell::new(HashMap::new());
        static USED: RefCell<BTreeSet<String>> = const { RefCell::new(BTreeSet::new()) };
        /// While restoring: each clip, by name.
        static LOADED: RefCell<HashMap<String, Arc<VertexAnimation>>> =
            RefCell::new(HashMap::new());
    }

    fn address(clip: &Arc<VertexAnimation>) -> usize {
        Arc::as_ptr(clip) as usize
    }

    /// Names the clips of a cache for saving.
    pub(super) fn saving(cache: &HashMap<String, Option<Arc<VertexAnimation>>>) {
        NAMES.with(|n| {
            let mut n = n.borrow_mut();
            n.clear();
            for (name, clip) in cache {
                if let Some(clip) = clip {
                    n.insert(address(clip), name.clone());
                }
            }
        });
        USED.with(|u| u.borrow_mut().clear());
    }

    /// The names the clips saved went by.
    pub(super) fn used() -> Vec<String> {
        USED.with(|u| u.borrow().iter().cloned().collect())
    }

    /// Makes these clips known by name, for restoring.
    pub(super) fn restoring(loaded: HashMap<String, Arc<VertexAnimation>>) {
        LOADED.with(|l| *l.borrow_mut() = loaded);
    }

    pub(super) fn done() {
        NAMES.with(|n| n.borrow_mut().clear());
        LOADED.with(|l| l.borrow_mut().clear());
    }

    fn name_of(clip: &Arc<VertexAnimation>) -> Result<String, String> {
        let name = NAMES
            .with(|n| n.borrow().get(&address(clip)).cloned())
            .ok_or("an animation of no known name")?;
        USED.with(|u| u.borrow_mut().insert(name.clone()));
        Ok(name)
    }

    fn named(name: &str) -> Result<Arc<VertexAnimation>, String> {
        LOADED
            .with(|l| l.borrow().get(name).cloned())
            .ok_or_else(|| format!("no animation {name}"))
    }

    pub mod option {
        use super::*;
        use serde::{Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            clip: &Option<Arc<VertexAnimation>>,
            s: S,
        ) -> Result<S::Ok, S::Error> {
            let name = clip.as_ref().map(name_of).transpose();
            name.map_err(serde::ser::Error::custom)?.serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Option<Arc<VertexAnimation>>, D::Error> {
            let name = Option::<String>::deserialize(d)?;
            name.map(|n| named(&n))
                .transpose()
                .map_err(serde::de::Error::custom)
        }
    }

    /// An animation's clips, by the frame each starts on.
    pub mod numbered {
        use super::*;
        use serde::{Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            clips: &[(u32, Arc<VertexAnimation>)],
            s: S,
        ) -> Result<S::Ok, S::Error> {
            let names: Result<Vec<(u32, String)>, String> =
                clips.iter().map(|(f, c)| Ok((*f, name_of(c)?))).collect();
            names.map_err(serde::ser::Error::custom)?.serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<Vec<(u32, Arc<VertexAnimation>)>, D::Error> {
            Vec::<(u32, String)>::deserialize(d)?
                .into_iter()
                .map(|(f, n)| Ok((f, named(&n)?)))
                .collect::<Result<_, String>>()
                .map_err(serde::de::Error::custom)
        }
    }

    /// A head's four turns, each with the angle it reaches.
    pub mod head {
        use super::*;
        use serde::{Deserializer, Serializer};

        pub fn serialize<S: Serializer>(
            clips: &[(Arc<VertexAnimation>, f32); 4],
            s: S,
        ) -> Result<S::Ok, S::Error> {
            let names: Result<Vec<(String, f32)>, String> =
                clips.iter().map(|(c, a)| Ok((name_of(c)?, *a))).collect();
            names.map_err(serde::ser::Error::custom)?.serialize(s)
        }

        pub fn deserialize<'de, D: Deserializer<'de>>(
            d: D,
        ) -> Result<[(Arc<VertexAnimation>, f32); 4], D::Error> {
            let found: Vec<(Arc<VertexAnimation>, f32)> = Vec::<(String, f32)>::deserialize(d)?
                .into_iter()
                .map(|(n, a)| Ok((named(&n)?, a)))
                .collect::<Result<_, String>>()
                .map_err(serde::de::Error::custom)?;
            found
                .try_into()
                .map_err(|_| serde::de::Error::custom("a head needs four turns"))
        }
    }
}

/// The cast as it stands, for a saved game.
#[derive(Serialize, Deserialize)]
pub struct CastSnapshot {
    /// The animations the figures are in the middle of, by name: loaded
    /// before the figures are read back.
    clips: Vec<String>,
    figures: serde_json::Value,
    cues: Vec<(f64, CueKind)>,
    last_update: f64,
    rng: u64,
}

/// One figure as it stands: everything that changes once the room is up.
#[derive(Serialize, Deserialize)]
struct FigureSnapshot {
    name: String,
    noun: Option<String>,
    fidgets: [Option<String>; 3],
    fidget: Option<Fidget>,
    pose: Pose,
    actor: Option<ActorPlacement>,
    world: Mat4,
    gas: Option<GasPlayer>,
    cleanup: VecDeque<String>,
    morph: Option<Morph>,
    painted: BTreeMap<usize, String>,
    hidden_parts: Vec<(usize, usize)>,
    shadow_on: bool,
    fixed_light: Option<[f32; 3]>,
    ambient_range: Option<f32>,
    floor_light: FloorLight,
    head: Option<HeadRig>,
    playing: Option<Playing>,
    walk: Option<Walk>,
    waiting_until: f64,
    visible: bool,
    face: Option<FaceSnapshot>,
}

/// A face as it stands; what it is made of comes from the character.
#[derive(Serialize, Deserialize)]
struct FaceSnapshot {
    expression: Expression,
    events: Vec<(f64, FaceChange)>,
    next_blink: f64,
    glance: Option<((f32, f32), f64)>,
    look_at: Option<(LookTarget, Option<f64>)>,
    jitter: (f32, f32),
    jitter_on: bool,
    next_jitter: f64,
}

impl FaceSnapshot {
    fn of(face: &FaceState) -> Self {
        Self {
            expression: face.expression.clone(),
            events: face.events.clone(),
            next_blink: face.next_blink,
            glance: face.glance,
            look_at: face.look_at.clone(),
            jitter: face.jitter,
            jitter_on: face.jitter_on,
            next_jitter: face.next_jitter,
        }
    }

    fn apply(self, face: &mut FaceState) {
        face.expression = self.expression;
        face.events = self.events;
        face.next_blink = self.next_blink;
        face.glance = self.glance;
        face.look_at = self.look_at;
        face.jitter = self.jitter;
        face.jitter_on = self.jitter_on;
        face.next_jitter = self.next_jitter;
        face.dirty = true;
    }
}

impl Cast {
    /// The cast as it stands, for a saved game.
    pub fn snapshot(&self) -> Result<CastSnapshot, String> {
        clips::saving(&self.clips);
        let figures: Vec<FigureSnapshot> = self
            .figures
            .iter()
            .map(|f| FigureSnapshot {
                name: f.name.clone(),
                noun: f.noun.clone(),
                fidgets: f.fidgets.clone(),
                fidget: f.fidget,
                pose: f.pose.clone(),
                actor: f.actor.clone(),
                world: f.world,
                gas: f.gas.clone(),
                cleanup: f.cleanup.clone(),
                morph: f.morph.clone(),
                painted: f.painted.clone(),
                hidden_parts: f.hidden_parts.iter().copied().collect(),
                shadow_on: f.shadow_on,
                fixed_light: f.fixed_light,
                ambient_range: f.ambient_range,
                floor_light: f.floor_light.clone(),
                head: f.head.clone(),
                playing: f.playing.clone(),
                walk: f.walk.clone(),
                waiting_until: f.waiting_until,
                visible: f.visible,
                face: f.face.as_ref().map(FaceSnapshot::of),
            })
            .collect();
        let value = serde_json::to_value(&figures);
        let used = clips::used();
        clips::done();
        Ok(CastSnapshot {
            clips: used,
            figures: value.map_err(|e| format!("the cast: {e}"))?,
            cues: self.cues.clone(),
            last_update: self.last_update,
            rng: self.rng,
        })
    }

    /// Puts the cast back as it was saved, over the cast the room's file
    /// gives (the same figures, as the room was entered under the same
    /// story).
    pub fn restore(&mut self, lib: &Library, saved: CastSnapshot) -> Result<(), String> {
        let mut loaded = HashMap::new();
        for name in &saved.clips {
            let clip = self
                .clip(lib, name)
                .ok_or_else(|| format!("no animation {name}"))?;
            loaded.insert(name.clone(), clip);
        }
        clips::restoring(loaded);
        let figures = serde_json::from_value::<Vec<FigureSnapshot>>(saved.figures);
        clips::done();
        let figures = figures.map_err(|e| format!("the cast: {e}"))?;
        if figures.len() != self.figures.len() {
            return Err(format!(
                "the room has {} figures; the save has {}",
                self.figures.len(),
                figures.len()
            ));
        }
        for (f, s) in self.figures.iter_mut().zip(figures) {
            if f.name != s.name {
                return Err(format!(
                    "the room has {} where the save has {}",
                    f.name, s.name
                ));
            }
            for name in s.painted.values() {
                self.paints
                    .entry(name.clone())
                    .or_insert_with(|| super::load_texture(lib, name));
            }
            f.repaint = s.painted.iter().map(|(i, n)| (*i, n.clone())).collect();
            f.painted = s.painted;
            f.noun = s.noun;
            f.fidgets = s.fidgets;
            f.fidget = s.fidget;
            f.pose = s.pose;
            f.actor = s.actor;
            f.world = s.world;
            f.gas = s.gas;
            f.cleanup = s.cleanup;
            f.morph = s.morph;
            f.hidden_parts = s.hidden_parts.into_iter().collect();
            f.shadow_on = s.shadow_on;
            f.fixed_light = s.fixed_light;
            f.ambient_range = s.ambient_range;
            f.floor_light = s.floor_light;
            f.head = s.head;
            f.playing = s.playing;
            f.walk = s.walk;
            f.waiting_until = s.waiting_until;
            f.visible = s.visible;
            if let (Some(face), Some(saved)) = (&mut f.face, s.face) {
                saved.apply(face);
            }
            f.dirty = true;
        }
        self.cues = saved.cues;
        self.last_update = saved.last_update;
        self.rng = saved.rng;
        Ok(())
    }
}
