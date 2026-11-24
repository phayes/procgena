//! Ready-made settings for common kinds of dead wood.
//!
//! [`DeadwoodPreset::all`] lists the built-in presets in menu order: logs,
//! then branches, then stumps, then pieces with leaves growing on them.
//! [`DeadwoodPreset::named`] looks one up by its key, and a `.deadwood.yml`
//! file loads as a preset with [`DeadwoodPreset::from_yaml`].

use std::borrow::Cow;

use crate::options::{
    BranchingOptions, DeadwoodOptions, EndKind, EndsOptions, HollowOptions, LeafOptions,
    MaterialOptions, PieceKind, RootOptions, StubOptions,
};

/// A named, ready-made piece of dead wood, such as a log brought down by
/// wind or a rotten stump.
///
/// Every built-in preset has a function of its own, such as
/// [`DeadwoodPreset::mossy_log`]. Clone its [`deadwood`](Self::deadwood)
/// options to edit them, or pass the preset to [`crate::Piece::from_preset`].
/// Change [`DeadwoodOptions::default_seed`] for another piece in the same
/// style.
#[derive(Debug, Clone, PartialEq)]
pub struct DeadwoodPreset {
    /// The preset's key, such as `"fresh_cut_log"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Fresh cut log"`.
    pub name: Cow<'static, str>,
    /// What the piece looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Logs"`, or `"File"` for a
    /// loaded file.
    pub group: Cow<'static, str>,
    /// The piece's settings. Built-in presets are always valid.
    pub deadwood: DeadwoodOptions,
}

procgena_core::catalogue! {
    DeadwoodPreset {
        deadwood: DeadwoodOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Logs" {
        fresh_cut_log {
            name: "Fresh cut log",
            description: "A recently felled log, sawn at both ends, bark intact.",
        } => fresh_cut_log();
        windthrow_log {
            name: "Windthrow log",
            description: "Windthrow: a trunk brought down by wind. It has a sawn base, a \
                jagged broken tip and branch stubs.",
        } => windthrow_log();
        mossy_log {
            name: "Mossy log",
            description: "A log that has lain for a few years (decay class 3): bark peeling, \
                sagging onto the ground. Pairs well with moss from procgena_clump.",
        } => mossy_log();
        hollow_log {
            name: "Hollow log",
            description: "A rotted-out log, hollow from end to end.",
        } => hollow_log();
        rotten_log {
            name: "Rotten log",
            description: "A soft, flattened, barkless log sinking into the forest floor \
                (decay class 4.5).",
        } => rotten_log();
        birch_log {
            name: "Birch log",
            description: "A thin log with pale bark.",
        } => birch_log();
    }
    "Branches" {
        fallen_limb {
            name: "Fallen limb",
            description: "A large fallen branch with side branches and twigs.",
        } => fallen_limb();
        dead_branch {
            name: "Dead branch",
            description: "A bare, broken dead branch.",
        } => dead_branch();
        stick {
            name: "Stick",
            description: "A single stick with a few twigs.",
        } => stick();
        twig {
            name: "Twig",
            description: "A small twig.",
        } => twig();
    }
    "Stumps" {
        felled_stump {
            name: "Felled stump",
            description: "A low stump from a sawn tree.",
        } => felled_stump();
        notched_stump {
            name: "Notched stump",
            description: "A stump with a chainsaw felling cut and buttresses: ridges where \
                the roots join the trunk.",
        } => notched_stump();
        snapped_stump {
            name: "Snapped stump",
            description: "A tall stump where the trunk snapped, with a splintered top.",
        } => snapped_stump();
        rotten_stump {
            name: "Rotten stump",
            description: "An old, hollow stump with no bark left.",
        } => rotten_stump();
    }
    "Leafy" {
        sprouting_stump {
            name: "Sprouting stump",
            description: "A sawn stump putting up clumps of leafy shoots around its top and \
                base.",
        } => sprouting_stump();
        seedling_log {
            name: "Seedling log",
            description: "A log a few years down, with maple seedlings growing from its \
                upper side.",
        } => seedling_log();
    }
}

/// A snapped end with the given jaggedness, splinters and slant.
fn snapped(jaggedness: f32, splinters: u32, one_sided: f32) -> EndKind {
    EndKind::Snapped {
        jaggedness,
        splinters,
        one_sided,
    }
}

/// Both ends of the main stem.
fn ends(base: EndKind, tip: EndKind) -> EndsOptions {
    EndsOptions { base, tip }
}

fn fresh_cut_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 11,
        decay: 0.5,
        length: 2.5,
        diameter: 0.4,
        taper: 0.9,
        ends: ends(EndKind::Saw { tilt: 3.0 }, EndKind::Saw { tilt: 5.0 }),
        stubs: StubOptions {
            per_metre: 1.2,
            ..Default::default()
        },
        ..DeadwoodOptions::default()
    }
}

fn windthrow_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 12,
        decay: 1.5,
        length: 4.0,
        diameter: 0.45,
        taper: 0.7,
        ends: ends(EndKind::Saw { tilt: 8.0 }, snapped(0.45, 8, 0.6)),
        stubs: StubOptions {
            per_metre: 2.0,
            ..Default::default()
        },
        ..DeadwoodOptions::default()
    }
}

fn mossy_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 13,
        decay: 3.0,
        length: 3.5,
        diameter: 0.4,
        ends: ends(snapped(0.3, 5, 0.3), EndKind::Saw { tilt: 10.0 }),
        ..DeadwoodOptions::default()
    }
}

fn hollow_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 14,
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
        ..DeadwoodOptions::default()
    }
}

fn rotten_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 15,
        decay: 4.5,
        length: 3.0,
        diameter: 0.45,
        ends: ends(snapped(0.3, 3, 0.2), snapped(0.3, 3, 0.4)),
        ..DeadwoodOptions::default()
    }
}

fn birch_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 16,
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
        ..DeadwoodOptions::default()
    }
}

fn fallen_limb() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 21,
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
        ..DeadwoodOptions::default()
    }
}

fn dead_branch() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 22,
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
        ..DeadwoodOptions::default()
    }
}

fn stick() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 23,
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
        ..DeadwoodOptions::default()
    }
}

fn twig() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 24,
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
        ..DeadwoodOptions::default()
    }
}

fn felled_stump() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 31,
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
        ..DeadwoodOptions::default()
    }
}

fn notched_stump() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 32,
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
        ..DeadwoodOptions::default()
    }
}

fn snapped_stump() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 33,
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
        ..DeadwoodOptions::default()
    }
}

fn rotten_stump() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 34,
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
        ..DeadwoodOptions::default()
    }
}

fn sprouting_stump() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 35,
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
        ..DeadwoodOptions::default()
    }
}

fn seedling_log() -> DeadwoodOptions {
    DeadwoodOptions {
        default_seed: 36,
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
        ..DeadwoodOptions::default()
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
    use std::collections::HashSet;

    use super::*;

    /// Checks that menu entries have unique keys and names and valid
    /// settings.
    #[test]
    fn presets_are_valid_and_named() {
        let mut keys = HashSet::new();
        let mut names = HashSet::new();
        for preset in DeadwoodPreset::all() {
            preset
                .deadwood
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", preset.name));
            assert!(!preset.name.is_empty());
            assert!(keys.insert(&preset.key), "{} is listed twice", preset.key);
            assert!(
                names.insert(&preset.name),
                "{} is listed twice",
                preset.name
            );
        }
    }

    /// Checks that every preset survives a trip through a `.deadwood.yml`
    /// file, written against no base and against another preset.
    #[test]
    fn every_preset_reads_back_from_its_file() {
        for preset in DeadwoodPreset::all() {
            let text = preset.to_yaml(None);
            let read = DeadwoodPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.deadwood, preset.deadwood, "{}", preset.key);
            assert_eq!(read.name, preset.name);
            let text = preset.to_yaml(Some(DeadwoodPreset::birch_log()));
            let read = DeadwoodPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.deadwood, preset.deadwood, "{}", preset.key);
        }
    }
}
