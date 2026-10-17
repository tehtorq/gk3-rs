//! The services the temple's machinery (TE3's pendulum, TE5's bridge) needs
//! from the room it works in, so that its logic can be kept, and tested,
//! apart from the world.

use glam::{Mat4, Vec3};

/// What a room's own machinery (TE3's pendulum, TE5's bridge) asks of the
/// room: the world, in practice.
pub trait Room {
    /// Plays an animation; its length in seconds (0 if there is none).
    fn play(&mut self, anim: &str, looping: bool) -> f32;
    /// Whether an animation is still moving anyone in the room.
    fn is_playing(&self, anim: &str) -> bool;
    /// Poses what an animation moves as in its first frame.
    fn sample(&mut self, anim: &str);
    fn cut(&mut self, camera: &str);
    /// Where a named spot of the room is, and its heading in degrees.
    fn spot(&self, name: &str) -> Option<(Vec3, f32)>;
    fn ego_at(&self) -> Vec3;
    /// Stands Gabriel somewhere, facing a heading in radians.
    fn put_ego(&mut self, at: Vec3, heading: f32);
    fn show(&mut self, model: &str, shown: bool);
    fn set_world(&mut self, model: &str, world: Mat4);
    fn set_flag(&mut self, flag: &str, on: bool);
    fn score(&mut self, event: &str);
    /// Runs a piece of script as an action.
    fn script(&mut self, text: &str);
    /// Keeps the camera the story's (or lets the player have it back).
    fn forced_cuts(&mut self, on: bool);
}
