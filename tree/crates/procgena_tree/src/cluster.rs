//! Place fruit and flower clusters on a tree's branches.
//!
//! This pass decides where each cluster grows, how it is turned and scaled,
//! and which of the tree's fruit specimens it shows. The clusters' own
//! stalks and items come from `procgena_fruit`.

use std::f32::consts::TAU;

use glam::{Quat, Vec3};
use procgena_core::Rng;
use procgena_core::hash::derive_seed;
use procgena_fruit::{Facing, choose_slot};

use crate::skeleton::{place, shuffled_indices};
use crate::{AttachedFruit, Attachment, Section, SkeletonBranch, TreeOptions, TreeType};

/// Which clusters of a tree: its fruit or its flowers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClusterPart {
    /// The fruit clusters ([`TreeOptions::fruit`]).
    Fruit,
    /// The flower clusters ([`TreeOptions::flowers`]).
    Flowers,
}

impl ClusterPart {
    /// Both parts, fruit first.
    pub const ALL: [Self; 2] = [Self::Fruit, Self::Flowers];

    /// The part's name, `"fruit"` or `"flowers"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Fruit => "fruit",
            Self::Flowers => "flowers",
        }
    }

    /// The tree's settings for this part, if it grows any.
    pub fn of(self, options: &TreeOptions) -> Option<&AttachedFruit> {
        match self {
            Self::Fruit => options.fruit.as_ref(),
            Self::Flowers => options.flowers.as_ref(),
        }
    }
}

/// One fruit or flower cluster placed on a branch.
///
/// The cluster itself is a `procgena_fruit` specimen built in its own frame;
/// this says where that frame sits on the tree. Positions use tree
/// coordinates, with the trunk base at the origin; lengths are in metres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkeletonCluster {
    /// Branch attachment identifying where this cluster grows.
    pub parent: Attachment,
    /// Where the cluster attaches: on the branch surface for side clusters,
    /// or at the branch end for tip clusters.
    pub origin: Vec3,
    /// Rotation from the cluster frame to tree coordinates. Local +Y follows
    /// the cluster's facing after random tilt. For hanging and upright
    /// clusters, local +Z points as nearly away from the branch as it can;
    /// for outward clusters, local +Z points as nearly up as it can.
    pub orientation: Quat,
    /// Scale from the cluster frame to tree coordinates.
    pub scale: f32,
    /// The slot of the fruit specimen this cluster shows, from 0 up to the
    /// tree's [`AttachedFruit::default_slots`].
    pub slot: u32,
}

impl SkeletonCluster {
    /// Moves `point` from the cluster frame into tree coordinates.
    pub fn to_tree(&self, point: Vec3) -> Vec3 {
        self.origin + self.orientation * (point * self.scale)
    }
}

/// Places clusters on eligible branches in their existing order, and picks
/// each one's slot. `tree_key` names the tree specimen, so that each
/// cluster's slot is fixed per tree specimen.
pub(crate) fn grow_clusters(
    branches: &[SkeletonBranch],
    options: &TreeOptions,
    attached: &AttachedFruit,
    tree_key: u32,
    mut rng: Rng,
) -> Vec<SkeletonCluster> {
    let mut clusters = Vec::new();
    let placement = &attached.placement;
    if !attached.grows() {
        return clusters;
    }
    let mut placer = Placer {
        attached,
        rng: &mut rng,
    };

    for (index, branch) in branches.iter().enumerate() {
        if branch.level < placement.min_level {
            continue;
        }
        let sections = &branch.sections;

        // Spread side clusters along the allowed branch length and around its surface.
        let count = placement.count + density_count(sections, attached, placer.rng);
        if count > 0 {
            let radial_offset = placer.rng.next_f32();
            let step = (1.0 - placement.start) / count as f32;
            for (i, slot) in shuffled_indices(placer.rng, count).into_iter().enumerate() {
                let along = placement.start + (i as f32 + placer.rng.next_f32()) * step;
                let at = place(sections, along, index);
                let jitter = placer.rng.range(-0.5, 0.5);
                let angle = TAU * (radial_offset + (slot as f32 + jitter) / count as f32);
                let out = at.orientation * Quat::from_rotation_y(angle) * Vec3::Z;
                let origin = at.origin + out * at.radius;
                clusters.push(placer.place(at.attachment, origin, out));
            }
        }

        // Add a possible tip cluster only where no branch continues from the tip.
        let continues =
            options.tree_type == TreeType::Deciduous && branch.level < options.branch.levels;
        if !continues && placement.tips > 0.0 && placer.rng.next_f32() < placement.tips {
            let last = sections.len() - 1;
            let tip = sections[last];
            let attachment = Attachment {
                branch: index,
                section: last,
                t: 0.0,
            };
            clusters.push(placer.place(attachment, tip.origin, tip.orientation * Vec3::Y));
        }
    }

    // Each cluster's slot is fixed by the tree specimen and the cluster's index.
    for (i, cluster) in (0u32..).zip(&mut clusters) {
        cluster.slot = choose_slot(derive_seed(tree_key, i), attached.default_slots);
    }
    clusters
}

/// Number of extra side clusters that [`ClusterPlacement::density`] adds to
/// one branch, from the branch length after `placement.start`. The
/// fractional part sets the chance of one more cluster. Draws nothing from
/// `rng` when the density is zero, so trees without density keep their
/// random sequence.
///
/// [`ClusterPlacement::density`]: crate::ClusterPlacement::density
fn density_count(sections: &[Section], attached: &AttachedFruit, rng: &mut Rng) -> u32 {
    let placement = &attached.placement;
    if placement.density <= 0.0 {
        return 0;
    }
    let length: f32 = sections
        .windows(2)
        .map(|pair| pair[0].origin.distance(pair[1].origin))
        .sum();
    let expected = placement.density * length * (1.0 - placement.start);
    (expected + rng.next_f32()).floor() as u32
}

/// Turns and scales clusters as they are placed.
struct Placer<'a> {
    attached: &'a AttachedFruit,
    rng: &'a mut Rng,
}

impl Placer<'_> {
    /// Places a cluster at `origin` on a branch attachment. `out` is the
    /// unit direction away from the branch surface, or along the branch at
    /// a tip. Its slot is chosen later.
    fn place(&mut self, parent: Attachment, origin: Vec3, out: Vec3) -> SkeletonCluster {
        let placement = &self.attached.placement;

        // Tilt the facing at random, then turn the frame about it.
        let facing = match self.attached.fruit.facing {
            Facing::Hang => Vec3::NEG_Y,
            Facing::Up => Vec3::Y,
            Facing::Outward => out,
        };
        let facing = self.tilt(facing, placement.facing_jitter.to_radians());
        let toward = match self.attached.fruit.facing {
            Facing::Hang | Facing::Up => out,
            Facing::Outward => Vec3::Y,
        };
        let orientation = frame(facing, toward);

        // Every cluster draws its scale, so a zero variance moves nothing else.
        let variance = placement.scale_variance;
        let scale = placement.scale * (1.0 + self.rng.range(-variance, variance));
        SkeletonCluster {
            parent,
            origin,
            orientation,
            scale,
            slot: 0,
        }
    }

    /// Tilts unit vector `direction` by a random angle from zero to
    /// `max_angle` radians, in a random direction around it.
    fn tilt(&mut self, direction: Vec3, max_angle: f32) -> Vec3 {
        let angle = max_angle * self.rng.next_f32();
        let azimuth = TAU * self.rng.next_f32();
        let (u, v) = direction.any_orthonormal_pair();
        let side = u * azimuth.cos() + v * azimuth.sin();
        direction * angle.cos() + side * angle.sin()
    }
}

/// The rotation whose local +Y is unit vector `facing` and whose local +Z
/// points as nearly toward `toward` as it can while staying at right
/// angles to `facing`. Where `toward` runs along `facing`, +Z is turned
/// from +Y the shortest way.
fn frame(facing: Vec3, toward: Vec3) -> Quat {
    let turn = Quat::from_rotation_arc(Vec3::Y, facing);
    let side = toward - facing * facing.dot(toward);
    if side.length_squared() < 1e-12 {
        return turn;
    }
    let z = turn * Vec3::Z;
    let angle = facing.dot(z.cross(side)).atan2(z.dot(side));
    Quat::from_axis_angle(facing, angle) * turn
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_faces_and_points_its_z_toward_the_target() {
        for (facing, toward) in [
            (Vec3::NEG_Y, Vec3::X),
            (Vec3::Y, Vec3::new(0.3, 0.2, -0.9).normalize()),
            (Vec3::new(1.0, 0.2, 0.0).normalize(), Vec3::Y),
        ] {
            let q = frame(facing, toward);
            assert!((q * Vec3::Y).abs_diff_eq(facing, 1e-5));
            let side = (toward - facing * facing.dot(toward)).normalize();
            assert!((q * Vec3::Z).abs_diff_eq(side, 1e-5), "{facing} {toward}");
        }
        // A target along the facing leaves the shortest turn.
        let q = frame(Vec3::Y, Vec3::Y);
        assert!(q.abs_diff_eq(Quat::IDENTITY, 1e-6));
    }
}
