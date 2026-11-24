//! Ground scatter: whole mushrooms and clusters spread over a
//! [`procgena_core::Ground`].

use std::f32::consts::TAU;

use glam::{DVec2, IVec2, Quat, Vec2, Vec3};
use procgena_core::hash::{pcg3d, seed_for};
use procgena_core::scatter::{Footprint, Placement, Rest, ScatterInput, place, steeper_than};
use procgena_core::{Rng, SpecimenId};

use crate::cluster::MushroomInstance;
use crate::mushroom::Mushroom;
use crate::options::MushroomOptions;
use crate::rng::{RngExt, SCATTER_SITES};
use crate::scatter::{MAX_SITES, MushroomCopy, MushroomScatter, ScatterError, ScatterOptions};

/// The most candidate spots a ground scatter's area may hold. Each lattice
/// cell is one candidate; see [`MushroomScatter::new`].
pub const MAX_CELLS: u64 = 16_000_000;

/// Smallest lattice cell, in metres, when
/// [`ScatterOptions::min_spacing`] is smaller.
const MIN_CELL: f32 = 0.1;

/// Points sampled round each stem's base to find the ground under it.
const BASE_RING: u32 = 6;

/// One lattice cell's candidate spot and what it would grow.
#[derive(Debug, Clone, Copy)]
struct Candidate {
    /// The cell.
    cell: IVec2,
    /// The spot, relative to the area's centre.
    at: Vec2,
    /// Rank from 0 to 1: the spot is kept where the density allows more
    /// than this, and gives way to lower-ranked neighbours.
    rank: f32,
    /// Turn about the vertical, in radians.
    yaw: f32,
    /// Size multiplier.
    scale: f32,
    /// Which specimen grows here.
    slot: u32,
}

impl MushroomScatter {
    /// Spreads whole mushrooms like `mushroom` over the ground and area of
    /// `input`: a single mushroom, or a whole cluster such as a tuft or a
    /// fairy ring when [`ClusterOptions::stems`](crate::ClusterOptions::stems)
    /// is more than one.
    ///
    /// Copies are drawn from [`ScatterOptions::specimens`] different
    /// specimens, each seeded from its [`SpecimenId`] with `input.seed` as
    /// the World seed. Neither [`ScatterOptions::seed`] nor the mushroom's
    /// own seed is used. Each copy gets its own turn and size, and every
    /// member of a cluster is set on the ground at its own spot, upright and
    /// sunk by [`ScatterOptions::sink`]; a member with no ground under it is
    /// left out, and the rest of the copy stays.
    ///
    /// Spots come from a lattice of cells anchored at the area's centre, one
    /// candidate per cell, [`min_spacing`](ScatterOptions::min_spacing)
    /// wide (at least 0.1 m), so the density can't exceed one per cell. A
    /// candidate is kept where the density and `input`'s mask allow it, then
    /// only where the ground is no steeper than
    /// [`max_slope`](ScatterOptions::max_slope), and it gives way to a
    /// neighbour closer than `min_spacing`. At most [`MAX_SITES`] copies are
    /// kept. Lowering the density or adding a mask only removes copies, and
    /// growing the area keeps the copies already there.
    ///
    /// Positions are relative to the area's centre: X and Z across, Y the
    /// ground's height. [`ScatterOptions::grows_on`],
    /// [`up`](ScatterOptions::up) and
    /// [`follow_surface`](ScatterOptions::follow_surface) are not used.
    ///
    /// # Errors
    ///
    /// Returns [`ScatterError::Options`] if a setting in `mushroom` or
    /// `options` is out of range, and [`ScatterError::TooLarge`] if the area
    /// holds more than [`MAX_CELLS`] cells.
    pub fn new(
        mushroom: &MushroomOptions,
        options: &ScatterOptions,
        input: &ScatterInput<'_>,
    ) -> Result<Self, ScatterError> {
        options.validate()?;
        mushroom.validate()?;

        // One whole mushroom or cluster per slot, with its bodies listed one
        // after another.
        let pool = mushroom.pool();
        let specimens = (0..options.specimens)
            .map(|slot| {
                Mushroom::new(MushroomOptions {
                    seed: SpecimenId::new(input.seed, pool, slot).seed(),
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

        // The cells that cover the area, anchored at its centre.
        let cell = options.min_spacing.max(MIN_CELL);
        // Count them in floating point first, so a huge or non-finite area
        // is refused rather than overflowing.
        let half = input.area.size.as_dvec2() * 0.5;
        let (lo, hi) = (
            (-half / f64::from(cell)).floor(),
            (half / f64::from(cell)).ceil(),
        );
        let span = (hi - lo).max(DVec2::ZERO);
        let count = span.x * span.y;
        if count.is_nan() || count > MAX_CELLS as f64 {
            let cells = if count.is_finite() {
                count as u64
            } else {
                u64::MAX
            };
            return Err(ScatterError::TooLarge { cells });
        }
        let (lo, hi) = (lo.as_ivec2(), hi.as_ivec2());

        // Keep each candidate that the density, its neighbours and the slope
        // allow, then the lowest-ranked ones up to the cap.
        let sites = Sites {
            options,
            input,
            cell,
            seed: seed_for(input.seed, SCATTER_SITES),
            specimens: options.specimens,
        };
        let mut kept = Vec::new();
        for y in lo.y..hi.y {
            for x in lo.x..hi.x {
                if let Some(c) = sites.keep(IVec2::new(x, y)) {
                    kept.push(c);
                }
            }
        }
        if kept.len() > MAX_SITES as usize {
            kept.sort_by(|a, b| a.rank.total_cmp(&b.rank));
            kept.truncate(MAX_SITES as usize);
            kept.sort_by_key(|c| (c.cell.y, c.cell.x));
        }

        // Set every member of each copy on the ground.
        let placement = Placement {
            conform: 0.0,
            rest: Rest::Lowest,
            embed: options.sink,
            max_bend: None,
        };
        let mut instances = Vec::new();
        let mut copies = Vec::new();
        for c in kept {
            let specimen = &specimens[c.slot as usize];
            let turn = Quat::from_rotation_y(c.yaw);
            let start = instances.len();
            let mut members = Vec::new();
            for (k, member) in specimen.members().iter().enumerate() {
                let scale = member.scale * c.scale;
                let offset = turn * (member.translation * c.scale);
                let at = c.at + Vec2::new(offset.x, offset.z);
                let body = &specimen.bodies()[member.variant as usize];
                let footprint = base_footprint(body.base_radius() * scale);
                let Some(placed) = place(
                    input.ground,
                    input.area.world(at),
                    0.0,
                    &footprint,
                    &placement,
                ) else {
                    continue;
                };
                instances.push(MushroomInstance {
                    variant: first_body[c.slot as usize] + member.variant,
                    translation: Vec3::new(at.x, placed.height, at.y),
                    rotation: turn * member.rotation,
                    scale,
                });
                members.push(k as u32);
            }
            if members.is_empty() {
                continue;
            }
            let height = input.sample(c.at).height;
            copies.push(MushroomCopy {
                slot: c.slot,
                cell: c.cell,
                translation: Vec3::new(c.at.x, height, c.at.y),
                yaw: c.yaw,
                scale: c.scale,
                instances: start..instances.len(),
                members,
            });
        }
        Ok(Self {
            bodies,
            instances,
            copies,
        })
    }
}

/// What a ground scatter needs to decide each lattice cell.
struct Sites<'a, 'b> {
    /// The scatter's settings.
    options: &'a ScatterOptions,
    /// The ground, mask and area.
    input: &'a ScatterInput<'b>,
    /// Width of each cell, in metres.
    cell: f32,
    /// Seed of every cell's random draws.
    seed: u32,
    /// How many specimens to choose from.
    specimens: u32,
}

impl Sites<'_, '_> {
    /// Returns the candidate of `cell`, drawn from the cell alone.
    fn candidate(&self, cell: IVec2) -> Candidate {
        let h = pcg3d([cell.x as u32, cell.y as u32, self.seed]);
        let mut rng = Rng::new(h[0]);
        let rank = rng.next_f32();
        let jitter = Vec2::new(rng.next_f32(), rng.next_f32());
        Candidate {
            cell,
            at: (cell.as_vec2() + jitter) * self.cell,
            rank,
            yaw: rng.range(0.0, TAU),
            scale: rng.range(self.options.scale_min, self.options.scale_max),
            slot: rng.int(0, self.specimens - 1),
        }
    }

    /// Returns true if the density and mask at `c` allow it.
    fn dense_enough(&self, c: &Candidate) -> bool {
        let allowed = self.options.density * self.cell * self.cell * self.input.density(c.at);
        c.rank < allowed
    }

    /// Returns the candidate of `cell` if it is kept: in the area, dense
    /// enough, not crowded by a lower-ranked neighbour, and on ground no
    /// steeper than the limit.
    fn keep(&self, cell: IVec2) -> Option<Candidate> {
        let c = self.candidate(cell);
        if !self.input.area.contains(c.at) || !self.dense_enough(&c) {
            return None;
        }

        // Give way to a closer, lower-ranked neighbour, whether or not it is
        // kept itself, so neither a smaller density, a mask nor a smaller
        // area can uncover a copy. Cells are at least `min_spacing` wide, so
        // only the eight around can be that close.
        let spacing = self.options.min_spacing;
        if spacing > 0.0 {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    if dx == 0 && dy == 0 {
                        continue;
                    }
                    let n = self.candidate(cell + IVec2::new(dx, dy));
                    if n.rank < c.rank && n.at.distance(c.at) < spacing {
                        return None;
                    }
                }
            }
        }

        (!steeper_than(&self.input.sample(c.at), self.options.max_slope)).then_some(c)
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
