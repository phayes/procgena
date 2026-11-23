//! Ready-made settings for common kinds of dead wood.

use crate::options::{
    BranchingOptions, DeadwoodOptions, EndKind, EndsOptions, HollowOptions, LeafOptions,
    MaterialOptions, PieceKind, RootOptions, StubOptions,
};

/// Named starting points for [`DeadwoodOptions`]. Use
/// [`options`](Self::options) to get the settings and edit them, or pass a
/// preset to [`crate::Piece::from_preset`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DeadwoodPreset {
    /// A recently felled log, sawn at both ends, bark intact.
    FreshCutLog,
    /// Windthrow: a trunk brought down by wind. It has a sawn base, a
    /// jagged broken tip and branch stubs.
    WindthrowLog,
    /// A log that has lain for a few years (decay class 3): bark peeling,
    /// sagging onto the ground. Pairs well with moss from `procgena_clump`.
    MossyLog,
    /// A rotted-out log, hollow from end to end.
    HollowLog,
    /// A soft, flattened, barkless log sinking into the forest floor (decay
    /// class 4.5).
    RottenLog,
    /// A thin log with pale bark.
    BirchLog,
    /// A large fallen branch with side branches and twigs.
    FallenLimb,
    /// A bare, broken dead branch.
    DeadBranch,
    /// A single stick with a few twigs.
    Stick,
    /// A small twig.
    Twig,
    /// A low stump from a sawn tree.
    FelledStump,
    /// A stump with a chainsaw felling cut and buttresses: ridges where the
    /// roots join the trunk.
    NotchedStump,
    /// A tall stump where the trunk snapped, with a splintered top.
    SnappedStump,
    /// An old, hollow stump with no bark left.
    RottenStump,
    /// A sawn stump putting up clumps of leafy shoots around its top and
    /// base.
    SproutingStump,
    /// A log a few years down, with maple seedlings growing from its upper
    /// side.
    SeedlingLog,
}

impl DeadwoodPreset {
    /// Every preset, in menu order.
    pub const ALL: [Self; 16] = [
        Self::FreshCutLog,
        Self::WindthrowLog,
        Self::MossyLog,
        Self::HollowLog,
        Self::RottenLog,
        Self::BirchLog,
        Self::FallenLimb,
        Self::DeadBranch,
        Self::Stick,
        Self::Twig,
        Self::FelledStump,
        Self::NotchedStump,
        Self::SnappedStump,
        Self::RottenStump,
        Self::SproutingStump,
        Self::SeedlingLog,
    ];

    /// A short display name.
    pub fn name(self) -> &'static str {
        match self {
            Self::FreshCutLog => "Fresh cut log",
            Self::WindthrowLog => "Windthrow log",
            Self::MossyLog => "Mossy log",
            Self::HollowLog => "Hollow log",
            Self::RottenLog => "Rotten log",
            Self::BirchLog => "Birch log",
            Self::FallenLimb => "Fallen limb",
            Self::DeadBranch => "Dead branch",
            Self::Stick => "Stick",
            Self::Twig => "Twig",
            Self::FelledStump => "Felled stump",
            Self::NotchedStump => "Notched stump",
            Self::SnappedStump => "Snapped stump",
            Self::RottenStump => "Rotten stump",
            Self::SproutingStump => "Sprouting stump",
            Self::SeedlingLog => "Seedling log",
        }
    }

    /// Returns the preset's settings, ready to edit or pass to
    /// [`crate::Piece::new`]. They are always valid.
    pub fn options(self) -> DeadwoodOptions {
        let log = DeadwoodOptions::default;
        let snapped = |jaggedness, splinters, one_sided| EndKind::Snapped {
            jaggedness,
            splinters,
            one_sided,
        };
        let ends = |base, tip| EndsOptions { base, tip };
        match self {
            Self::FreshCutLog => DeadwoodOptions {
                seed: 11,
                decay: 0.5,
                length: 2.5,
                diameter: 0.4,
                taper: 0.9,
                ends: ends(EndKind::Saw { tilt: 3.0 }, EndKind::Saw { tilt: 5.0 }),
                stubs: StubOptions {
                    per_metre: 1.2,
                    ..Default::default()
                },
                ..log()
            },
            Self::WindthrowLog => DeadwoodOptions {
                seed: 12,
                decay: 1.5,
                length: 4.0,
                diameter: 0.45,
                taper: 0.7,
                ends: ends(EndKind::Saw { tilt: 8.0 }, snapped(0.45, 8, 0.6)),
                stubs: StubOptions {
                    per_metre: 2.0,
                    ..Default::default()
                },
                ..log()
            },
            Self::MossyLog => DeadwoodOptions {
                seed: 13,
                decay: 3.0,
                length: 3.5,
                diameter: 0.4,
                ends: ends(snapped(0.3, 5, 0.3), EndKind::Saw { tilt: 10.0 }),
                ..log()
            },
            Self::HollowLog => DeadwoodOptions {
                seed: 14,
                decay: 3.5,
                length: 3.0,
                diameter: 0.5,
                taper: 0.85,
                ends: ends(snapped(0.25, 4, 0.3), snapped(0.25, 4, 0.5)),
                hollow: HollowOptions {
                    enabled: true,
                    radius: 0.65,
                    open_base: true,
                    open_tip: true,
                    ..Default::default()
                },
                ..log()
            },
            Self::RottenLog => DeadwoodOptions {
                seed: 15,
                decay: 4.5,
                length: 3.0,
                diameter: 0.45,
                ends: ends(snapped(0.3, 3, 0.2), snapped(0.3, 3, 0.4)),
                ..log()
            },
            Self::BirchLog => DeadwoodOptions {
                seed: 16,
                decay: 2.0,
                length: 2.0,
                diameter: 0.22,
                taper: 0.85,
                ends: ends(EndKind::Saw { tilt: 4.0 }, EndKind::Saw { tilt: 12.0 }),
                material: MaterialOptions {
                    bark: "#d9d4c7".into(),
                    bark_weathered: "#a9a398".into(),
                    sapwood: "#e3d6bd".into(),
                    heartwood: "#c9a982".into(),
                    bark_texture: None,
                    ..Default::default()
                },
                ..log()
            },
            Self::FallenLimb => DeadwoodOptions {
                seed: 21,
                kind: PieceKind::Branch,
                decay: 1.0,
                length: 2.5,
                diameter: 0.12,
                taper: 0.3,
                ends: ends(snapped(0.4, 4, 0.5), EndKind::Taper),
                stubs: no_stubs(),
                branching: BranchingOptions {
                    levels: 2,
                    children: 5,
                    twigs: 5,
                    ..Default::default()
                },
                ..log()
            },
            Self::DeadBranch => DeadwoodOptions {
                seed: 22,
                kind: PieceKind::Branch,
                decay: 2.5,
                length: 1.8,
                diameter: 0.08,
                taper: 0.4,
                ends: ends(snapped(0.4, 3, 0.5), snapped(0.3, 2, 0.5)),
                stubs: no_stubs(),
                branching: BranchingOptions {
                    levels: 1,
                    children: 4,
                    twigs: 3,
                    ..Default::default()
                },
                ..log()
            },
            Self::Stick => DeadwoodOptions {
                seed: 23,
                kind: PieceKind::Branch,
                decay: 1.5,
                length: 0.7,
                diameter: 0.025,
                taper: 0.5,
                ends: ends(snapped(0.35, 2, 0.4), EndKind::Taper),
                stubs: no_stubs(),
                branching: BranchingOptions {
                    levels: 0,
                    twigs: 2,
                    twig_keep: Some(1.0),
                    ..Default::default()
                },
                ..log()
            },
            Self::Twig => DeadwoodOptions {
                seed: 24,
                kind: PieceKind::Branch,
                decay: 1.0,
                length: 0.3,
                diameter: 0.008,
                taper: 0.4,
                ends: ends(snapped(0.3, 1, 0.4), EndKind::Taper),
                stubs: no_stubs(),
                branching: BranchingOptions {
                    levels: 0,
                    twigs: 3,
                    twig_keep: Some(1.0),
                    twig_length: 0.3,
                    ..Default::default()
                },
                ..log()
            },
            Self::FelledStump => DeadwoodOptions {
                seed: 31,
                kind: PieceKind::Stump,
                material: stump_material(),
                decay: 0.5,
                length: 0.5,
                diameter: 0.5,
                taper: 0.95,
                ends: ends(EndKind::Saw { tilt: 0.0 }, EndKind::Saw { tilt: 2.0 }),
                stubs: no_stubs(),
                roots: RootOptions {
                    flare: 0.45,
                    ..Default::default()
                },
                ..log()
            },
            Self::NotchedStump => DeadwoodOptions {
                seed: 32,
                kind: PieceKind::Stump,
                material: stump_material(),
                decay: 1.0,
                length: 0.7,
                diameter: 0.55,
                taper: 0.95,
                ends: ends(
                    EndKind::Saw { tilt: 0.0 },
                    EndKind::Chainsaw {
                        tilt: 5.0,
                        step: 0.12,
                    },
                ),
                stubs: no_stubs(),
                roots: RootOptions {
                    flare: 0.6,
                    lobes: 6,
                    lobe_depth: 0.7,
                    count: 5,
                    ..Default::default()
                },
                ..log()
            },
            Self::SnappedStump => DeadwoodOptions {
                seed: 33,
                kind: PieceKind::Stump,
                material: stump_material(),
                decay: 2.0,
                length: 1.4,
                diameter: 0.45,
                taper: 0.9,
                ends: ends(EndKind::Saw { tilt: 0.0 }, snapped(0.5, 10, 0.7)),
                stubs: StubOptions {
                    per_metre: 1.0,
                    ..Default::default()
                },
                ..log()
            },
            Self::RottenStump => DeadwoodOptions {
                seed: 34,
                kind: PieceKind::Stump,
                material: stump_material(),
                decay: 4.2,
                length: 0.6,
                diameter: 0.5,
                taper: 0.9,
                ends: ends(EndKind::Saw { tilt: 0.0 }, snapped(0.15, 4, 0.5)),
                stubs: no_stubs(),
                hollow: HollowOptions {
                    enabled: true,
                    radius: 0.6,
                    open_base: false,
                    open_tip: true,
                    depth: 0.8,
                    roughness: 0.3,
                },
                roots: RootOptions {
                    count: 3,
                    ..Default::default()
                },
                ..log()
            },
            Self::SproutingStump => DeadwoodOptions {
                seed: 35,
                kind: PieceKind::Stump,
                material: stump_material(),
                decay: 1.0,
                length: 0.6,
                diameter: 0.55,
                taper: 0.95,
                ends: ends(EndKind::Saw { tilt: 0.0 }, EndKind::Saw { tilt: 3.0 }),
                stubs: no_stubs(),
                leaves: LeafOptions {
                    per_metre: 12.0,
                    cards: 5,
                    size: 0.3,
                    spread: 40.0,
                    up: 0.75,
                    top: 0.0,
                    ends: 0.8,
                    ..Default::default()
                },
                ..log()
            },
            Self::SeedlingLog => DeadwoodOptions {
                seed: 36,
                decay: 2.5,
                length: 2.5,
                diameter: 0.35,
                ends: ends(EndKind::Saw { tilt: 4.0 }, EndKind::Saw { tilt: 8.0 }),
                stubs: StubOptions {
                    per_metre: 0.8,
                    ..Default::default()
                },
                leaves: LeafOptions {
                    per_metre: 2.5,
                    cards: 3,
                    size: 0.22,
                    spread: 25.0,
                    lean: 10.0,
                    up: 0.9,
                    top: 1.0,
                    ends: 0.0,
                    ..Default::default()
                },
                ..log()
            },
        }
    }
}

/// Stub settings with no stubs.
fn no_stubs() -> StubOptions {
    StubOptions {
        per_metre: 0.0,
        ..Default::default()
    }
}

/// Material for stumps: the default, with the fresher end-grain photo on
/// a sawn top.
fn stump_material() -> MaterialOptions {
    MaterialOptions {
        end_texture: Some("TreeEnd003".into()),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid_and_named() {
        for preset in DeadwoodPreset::ALL {
            preset
                .options()
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", preset.name()));
            assert!(!preset.name().is_empty());
        }
    }
}
