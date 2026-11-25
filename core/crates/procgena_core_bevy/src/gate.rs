//! Baking only the impostors a camera can come far enough away to see.
//!
//! An impostor takes a bake (or a disk read) before it can be drawn, and is
//! only drawn when its copy is small on screen. When no camera can ever be
//! that far from a copy, as in a viewer whose orbit camera stays close, the
//! bake is wasted. Set [`FarthestViewDistance`] to the farthest any camera
//! can be from what it draws, and assets ask [`ImpostorGate`] before
//! requesting an impostor. Until the gate lets one through, its copies draw
//! their lower mesh until they are culled. Entities marked [`BakeAhead`]
//! bake regardless, for baking ahead of need.

use bevy::ecs::component::Component;
use bevy::ecs::resource::Resource;
use bevy::ecs::system::{Res, SystemParam};
use procgena_core::Quality;
use procgena_core::chain::{DistanceChain, Step, ViewScale};

use crate::LodQuality;

/// The farthest, in metres, any camera can be from a copy it draws.
///
/// Assets use it to skip baking impostors that can never be on screen (see
/// [`ImpostorGate`]). It defaults to [`UNLIMITED`](Self::UNLIMITED), which
/// bakes every impostor. A viewer whose camera can only orbit close to its
/// subject sets it to the camera's farthest distance plus the subject's
/// radius. Raising it later starts the impostors that become reachable;
/// lowering it never removes an impostor already drawn.
#[derive(Resource, Debug, Clone, Copy, PartialEq)]
pub struct FarthestViewDistance(pub f32);

impl FarthestViewDistance {
    /// No limit: cameras can be anywhere, so every impostor is baked.
    pub const UNLIMITED: Self = Self(f32::INFINITY);
}

impl Default for FarthestViewDistance {
    fn default() -> Self {
        Self::UNLIMITED
    }
}

/// Bakes the impostor of whatever this entity draws, however far cameras
/// can be.
///
/// Put it on an entity that is spawned to bake impostors ahead of need,
/// such as a hidden copy of each preset behind a loading screen. Assets
/// treat a specimen as reachable when any entity drawing it carries this
/// marker, whatever [`ImpostorGate`] says.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BakeAhead;

/// Whether a copy `size` metres across (its bounding-sphere diameter) can
/// be drawn as an impostor when no camera is farther than `farthest` metres
/// away: whether the impostor of the default [`DistanceChain`] starts fading
/// in within `farthest`, seen through a [`ViewScale::DEFAULT`] camera at
/// `quality`.
///
/// Many copies share one impostor; ask with the smallest of their sizes,
/// since smaller copies reach their impostor closer to the camera.
pub fn impostor_can_show(size: f32, quality: Quality, farthest: f32) -> bool {
    let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
    let ranges = DistanceChain::DEFAULT.ranges(factor, quality);
    ranges[Step::Impostor.index()].fade_in.start <= farthest
}

/// Asks whether an impostor can be on screen before it is baked.
///
/// Use it in a system that requests impostors, and skip the request when
/// [`can_show`](Self::can_show) says no. Keep asking every frame: a larger
/// [`FarthestViewDistance`] or a lower [`LodQuality`] can let it through
/// later. An impostor that is already baked or loaded costs nothing to draw,
/// so use it whatever the gate says. Entities marked [`BakeAhead`] bake
/// their impostor whatever the gate says.
///
/// ```
/// use bevy::prelude::*;
/// use procgena_core_bevy::ImpostorGate;
///
/// fn request_impostors(gate: ImpostorGate) {
///     // A rock 1.5 m across.
///     if gate.can_show(1.5) {
///         // Ask for its impostor here.
///     }
/// }
/// # bevy::ecs::system::assert_is_system(request_impostors);
/// ```
#[derive(SystemParam)]
pub struct ImpostorGate<'w> {
    quality: Res<'w, LodQuality>,
    farthest: Res<'w, FarthestViewDistance>,
}

impl ImpostorGate<'_> {
    /// Whether a copy `size` metres across can come far enough from every
    /// camera to be drawn as its impostor; see [`impostor_can_show`].
    pub fn can_show(&self, size: f32) -> bool {
        impostor_can_show(size, self.quality.0, self.farthest.0)
    }
}

#[cfg(test)]
mod tests {
    use bevy::prelude::*;

    use super::*;

    /// The distance where a copy `size` metres across starts fading into
    /// its impostor at `quality`.
    fn impostor_start(size: f32, quality: Quality) -> f32 {
        let factor = DistanceChain::factor(size, ViewScale::DEFAULT);
        DistanceChain::DEFAULT.ranges(factor, quality)[Step::Impostor.index()]
            .fade_in
            .start
    }

    #[test]
    fn an_impostor_shows_only_within_the_farthest_distance() {
        let start = impostor_start(2.0, Quality::High);
        assert!(!impostor_can_show(2.0, Quality::High, start * 0.99));
        assert!(impostor_can_show(2.0, Quality::High, start));
        assert!(impostor_can_show(2.0, Quality::High, f32::INFINITY));
    }

    #[test]
    fn smaller_copies_and_lower_quality_reach_the_impostor_sooner() {
        let farthest = impostor_start(2.0, Quality::High) * 0.9;
        assert!(!impostor_can_show(2.0, Quality::High, farthest));
        assert!(impostor_can_show(1.0, Quality::High, farthest));
        assert!(impostor_can_show(2.0, Quality::Low, farthest));
    }

    #[test]
    fn the_gate_reads_the_quality_and_the_farthest_distance() {
        #[derive(Resource)]
        struct Answer(bool);

        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .init_resource::<LodQuality>()
            .init_resource::<FarthestViewDistance>()
            .insert_resource(Answer(false))
            .add_systems(Update, |gate: ImpostorGate, mut answer: ResMut<Answer>| {
                answer.0 = gate.can_show(2.0);
            });

        app.update();
        assert!(app.world().resource::<Answer>().0, "unlimited by default");

        let start = impostor_start(2.0, Quality::High);
        app.insert_resource(FarthestViewDistance(start * 0.5));
        app.update();
        assert!(!app.world().resource::<Answer>().0);

        app.insert_resource(FarthestViewDistance(start));
        app.update();
        assert!(app.world().resource::<Answer>().0);
    }
}
