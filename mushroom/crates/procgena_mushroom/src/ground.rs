//! Ground scatter: whole mushrooms and clusters spread over a
//! [`procgena_core::Ground`].

use std::f32::consts::TAU;

use glam::{DVec2, Quat, Vec2, Vec3};
use procgena_core::hash::seed_for;
use procgena_core::scatter::lattice::{self, Fixed2};
use procgena_core::scatter::{
    DEFAULT_SPACING_ROUNDS, Disc, Footprint, Lattice, Rank, Reach, Scatter, ScatterCopy,
    ScatterInput, ScatterSlot, Spaced, keep_spaced, place, steeper_than,
};
use procgena_core::{CellId, Quality, SpecimenId};

use crate::cluster::MushroomInstance;
use crate::detail::default_lod;
use crate::mushroom::Mushroom;
use crate::options::MushroomOptions;
use crate::rng::SCATTER_SITES;
use crate::scatter::{MushroomCopy, MushroomScatter, ScatterError, ScatterOptions};

/// The most candidate spots a ground scatter's area may hold, counting the
/// margin its spacing needs. Each lattice cell is one candidate; see
/// [`MushroomScatter::new`].
pub const MAX_CELLS: u64 = 16_000_000;

/// Smallest lattice cell, in metres, when
/// [`ScatterOptions::min_spacing`] is smaller.
const MIN_CELL: f32 = 0.1;

/// Points sampled round each stem's base to find the ground under it.
const BASE_RING: u32 = 6;

/// Salts that give each draw of one candidate its own value.
const SALT_YAW: u32 = 0x9e37_79b9;
const SALT_SCALE: u32 = 0x6a09_e667;
const SALT_SLOT: u32 = 0xbb67_ae85;

/// What a scatter on the ground places with: its settings and specimens.
#[derive(Debug, Clone)]
pub(crate) struct GroundSpecimens {
    mushroom: MushroomOptions,
    options: ScatterOptions,
    /// One whole mushroom or cluster per slot.
    specimens: Vec<Mushroom>,
    /// Where each specimen's bodies start in the scatter's bodies.
    first_body: Vec<u32>,
}

/// A copy that passed the density, slope and ground checks, waiting for the
/// spacing check.
struct Candidate {
    position: Fixed2,
    rank: Rank,
    copy: MushroomCopy,
    instances: Vec<MushroomInstance>,
}

impl Spaced for Candidate {
    fn position(&self) -> Fixed2 {
        self.position
    }

    fn rank(&self) -> Rank {
        self.rank
    }
}

impl MushroomScatter {
    /// Spreads whole mushrooms like `mushroom` over the ground and area of
    /// `input`: a single mushroom, or a whole cluster such as a tuft or a
    /// fairy ring when [`ClusterOptions::stems`](crate::ClusterOptions::stems)
    /// is more than one.
    ///
    /// Copies are drawn from [`ScatterOptions::specimens`] different
    /// specimens, each seeded from its [`SpecimenId`] with `input.seed` as
    /// the World seed. Neither [`ScatterOptions::default_seed`] nor the
    /// mushroom's own [`default_seed`](MushroomOptions::default_seed) is
    /// used. Each copy gets its own turn and size, and every member of a
    /// cluster is set on the ground at its own spot with
    /// [`ScatterOptions::placement`], or the species'
    /// [`ground_placement`](MushroomOptions::ground_placement) (upright,
    /// resting on the lowest ground under its stem). A member with no ground
    /// under it, ground bent more than the placement allows, or a base
    /// overlapping a footprint the input keeps clear, is left out, and the
    /// rest of the copy stays.
    ///
    /// Spots come from a lattice of cells fixed to the World, one candidate
    /// per cell, [`min_spacing`](ScatterOptions::min_spacing) wide (at least
    /// 0.1 m), so the density can't exceed one per cell. A candidate is kept
    /// where the density and `input`'s mask allow it, then only where the
    /// ground is no steeper than [`max_slope`](ScatterOptions::max_slope)
    /// and at least one member stands, and copies closer than `min_spacing`
    /// are thinned by rank. Lowering the density or adding a mask only
    /// removes copies, and an area holds the same copies, with the same
    /// addresses, as any larger area around it.
    ///
    /// Positions are relative to the area's centre: X and Z across, Y the
    /// ground's height. The area holds the copies whose cell's corner lies
    /// inside it, so a copy may stand up to a cell outside it.
    /// [`ScatterOptions::grows_on`], [`up`](ScatterOptions::up) and
    /// [`follow_surface`](ScatterOptions::follow_surface) are not used.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting in `mushroom` or
    /// `options` is out of range, and [`ScatterError::TooLarge`] if the area
    /// and its margin hold more than [`MAX_CELLS`] cells.
    pub fn new(
        mushroom: &MushroomOptions,
        options: &ScatterOptions,
        input: &ScatterInput<'_>,
    ) -> Result<Self, ScatterError> {
        let mut scatter = Self::prepare(mushroom, options, input.seed)?;
        let (copies, instances) = scatter.copies_over(input)?;
        scatter.copies = copies;
        scatter.instances = instances;
        Ok(scatter)
    }

    /// Builds the specimens of a ground scatter of `mushroom` in a World
    /// seeded with `world_seed`, without placing any copies: place them as
    /// a [`Scatter`].
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting in `mushroom` or
    /// `options` is out of range.
    pub fn prepare(
        mushroom: &MushroomOptions,
        options: &ScatterOptions,
        world_seed: u32,
    ) -> Result<Self, ScatterError> {
        options.validate()?;
        mushroom.validate()?;

        // One whole mushroom or cluster per slot, with its bodies listed one
        // after another.
        let pool = mushroom.pool();
        let specimens = (0..options.specimens)
            .map(|slot| {
                Mushroom::new(MushroomOptions {
                    default_seed: SpecimenId::new(world_seed, pool, slot).seed(),
                    ..mushroom.clone()
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut first_body = Vec::with_capacity(specimens.len());
        let mut bodies = Vec::new();
        for specimen in &specimens {
            first_body.push(bodies.len() as u32);
            bodies.extend(specimen.bodies().iter().cloned());
        }
        Ok(Self {
            bodies,
            instances: Vec::new(),
            copies: Vec::new(),
            ground: Some(GroundSpecimens {
                mushroom: mushroom.clone(),
                options: options.clone(),
                specimens,
                first_body,
            }),
        })
    }

    /// The copies of `input`'s area, and their members' instances, which
    /// each copy's [`instances`](MushroomCopy::instances) range indexes.
    fn copies_over(
        &self,
        input: &ScatterInput<'_>,
    ) -> Result<(Vec<MushroomCopy>, Vec<MushroomInstance>), ScatterError> {
        let ground = self.ground.as_ref().ok_or(ScatterError::HostOnly)?;
        let options = &ground.options;

        // Count the cells to look at first, so a huge or non-finite area is
        // refused rather than overflowing.
        if !input.area.size.is_finite() || !input.area.centre.is_finite() {
            return Err(ScatterError::TooLarge { cells: u64::MAX });
        }
        let lattice = self.lattice();
        let reach = self.reach();
        let own = input.area.bounds();
        let window = reach.window(own);
        let count = lattice.count_in(window);
        if count > MAX_CELLS {
            return Err(ScatterError::TooLarge { cells: count });
        }

        // Find every candidate near enough to decide the area's own.
        let cell = f64::from(lattice.cell_metres());
        let chance = lattice::chance(f64::from(options.density) * cell * cell);
        let seed = seed_for(input.seed, SCATTER_SITES);
        let mut found = Vec::new();
        for cell in lattice.cells_in(window) {
            let h = lattice.hash(seed, cell);
            let at = lattice.position(h, cell);
            let local = input.local_density(at);
            let rank = lattice::rank(h);
            if !lattice::dense_enough(rank, chance, local)
                || steeper_than(&input.sample_at(at), options.max_slope)
            {
                continue;
            }
            if let Some((copy, instances)) = ground.grow(cell, h, at, input) {
                found.push(Candidate {
                    position: at,
                    rank: Rank {
                        tier: 0,
                        rank,
                        local,
                        cell,
                        index: 0,
                    },
                    copy,
                    instances,
                });
            }
        }

        // Thin copies closer than the spacing by rank, and keep the area's
        // own, with their members' instances listed in order.
        let spacing = reach.spacing_fixed();
        let keep = keep_spaced(&found, reach.rounds, spacing, |a, b| {
            a.position.distance_squared(b.position) < i128::from(spacing) * i128::from(spacing)
        });
        let mut copies = Vec::new();
        let mut instances = Vec::new();
        for (c, _) in found
            .into_iter()
            .zip(keep)
            .filter(|(c, keep)| *keep && own.contains(lattice.corner(c.rank.cell)))
        {
            let mut copy = c.copy;
            copy.instances = instances.len()..instances.len() + c.instances.len();
            instances.extend(c.instances);
            copies.push(copy);
        }
        Ok((copies, instances))
    }
}

impl GroundSpecimens {
    /// Grows the copy of `cell`, hashed to `h`, at World position `at`: picks
    /// its specimen, turn and size, and sets each member on the ground.
    /// Returns `None` where no member stands.
    fn grow(
        &self,
        cell: CellId,
        h: u32,
        at: Fixed2,
        input: &ScatterInput<'_>,
    ) -> Option<(MushroomCopy, Vec<MushroomInstance>)> {
        let options = &self.options;
        let yaw = lattice::unit(h, SALT_YAW) * TAU;
        let scale = options.scale_min
            + (options.scale_max - options.scale_min) * lattice::unit(h, SALT_SCALE);
        let slot = ((lattice::unit(h, SALT_SLOT) * options.specimens as f32) as u32)
            .min(options.specimens - 1);
        let specimen = &self.specimens[slot as usize];
        let turn = Quat::from_rotation_y(yaw);
        let placement = options.placement_for(&self.mushroom);
        let world = at.to_world();
        let local = at.relative_to(input.area.centre);

        // Set every member on the ground, keeping it off the footprints above.
        let mut instances = Vec::new();
        let mut members = Vec::new();
        let mut footprint = Vec::new();
        for (k, member) in specimen.members().iter().enumerate() {
            let member_scale = member.scale * scale;
            let offset = turn * (member.translation * scale);
            let spot = world + DVec2::new(f64::from(offset.x), f64::from(offset.z));
            let body = &specimen.bodies()[member.variant as usize];
            let radius = body.base_radius() * member_scale;
            let disc = Disc::new(spot, radius);
            if input.excluded(&[disc]) {
                continue;
            }
            let Some(placed) = place(input.ground, spot, 0.0, &base_footprint(radius), &placement)
            else {
                continue;
            };
            let here = local + Vec2::new(offset.x, offset.z);
            instances.push(MushroomInstance {
                variant: self.first_body[slot as usize] + member.variant,
                translation: Vec3::new(here.x, placed.height, here.y),
                rotation: turn * member.rotation,
                scale: member_scale,
            });
            members.push(k as u32);
            footprint.push(disc);
        }
        if members.is_empty() {
            return None;
        }
        let height = input.sample_at(at).height;
        Some((
            MushroomCopy {
                slot,
                address: input.prefix.copy(cell, 0),
                translation: Vec3::new(local.x, height, local.y),
                yaw,
                scale,
                instances: 0..0,
                members,
                footprint,
            },
            instances,
        ))
    }

    /// How far from a copy's centre its members' bases reach, at its largest
    /// size, in metres.
    fn widest(&self) -> f32 {
        let scale = self.options.scale_max;
        self.specimens
            .iter()
            .flat_map(|specimen| {
                specimen.members().iter().map(|m| {
                    let body = &specimen.bodies()[m.variant as usize];
                    (m.translation.with_y(0.0).length() + body.base_radius() * m.scale) * scale
                })
            })
            .fold(0.0, f32::max)
    }
}

/// A scatter on the ground places its copies on a lattice fixed to the
/// World, one candidate per cell. Its slots are its specimens, from the
/// World seed given to [`slots`](Scatter::slots); the specimens it draws are
/// the ones it was built with (see [`MushroomScatter::prepare`]). A scatter
/// grown on a host with [`MushroomScatter::on`] places nothing: its
/// [`place`](Scatter::place) returns [`ScatterError::HostOnly`].
impl Scatter for MushroomScatter {
    fn lattice(&self) -> Lattice {
        let spacing = self.ground.as_ref().map_or(0.0, |g| g.options.min_spacing);
        Lattice::new(spacing.max(MIN_CELL))
    }

    fn reach(&self) -> Reach {
        let cell = self.lattice().cell_metres();
        let (spacing, footprint) = self
            .ground
            .as_ref()
            .map_or((0.0, 0.0), |g| (g.options.min_spacing, g.widest()));
        Reach {
            rounds: DEFAULT_SPACING_ROUNDS,
            spacing,
            displacement: cell,
            footprint,
        }
    }

    fn draw_range(&self) -> f32 {
        // The default detail levels' cull distance, for the largest copy.
        let Some(ground) = &self.ground else {
            return 0.0;
        };
        let lod = default_lod();
        let size = ground.mushroom.size * ground.options.scale_max;
        lod.cull_distance.unwrap_or(f32::INFINITY) * lod.factor(size, Quality::High)
    }

    fn slots(&self, world_seed: u32) -> Vec<ScatterSlot> {
        let Some(ground) = &self.ground else {
            return Vec::new();
        };
        let pool = ground.mushroom.pool();
        (0..ground.options.specimens)
            .map(|slot| ScatterSlot {
                entry: 0,
                specimen: SpecimenId::new(world_seed, pool, slot),
            })
            .collect()
    }

    fn place(
        &self,
        input: &ScatterInput<'_>,
    ) -> Result<Vec<ScatterCopy>, procgena_core::scatter::ScatterError> {
        let (copies, _) = self.copies_over(input)?;
        Ok(copies
            .into_iter()
            .map(|copy| {
                let rotation = Quat::from_rotation_y(copy.yaw);
                ScatterCopy {
                    address: copy.address,
                    slot: copy.slot,
                    transform: glam::Affine3A::from_scale_rotation_translation(
                        Vec3::splat(copy.scale),
                        rotation,
                        copy.translation,
                    ),
                    footprint: copy.footprint,
                }
            })
            .collect())
    }
}

/// Returns the footprint of a stem base of `radius` metres: a ring of
/// points where it meets flat ground.
fn base_footprint(radius: f32) -> Footprint {
    let contacts = (0..BASE_RING)
        .map(|i| {
            let angle = i as f32 / BASE_RING as f32 * TAU;
            Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius)
        })
        .collect();
    Footprint {
        contacts,
        extra: Vec::new(),
        size: radius,
    }
}
