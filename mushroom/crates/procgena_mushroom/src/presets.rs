//! Ready-made mushroom species to start from.
//!
//! [`MushroomPreset::all`] lists the built-in presets in menu order, and
//! [`MushroomPreset::named`] looks one up by its key. A `.mushroom.yml` file
//! loads as a preset with [`MushroomPreset::from_yaml`].

use std::borrow::Cow;

use crate::cluster::{ClusterLayout, ClusterOptions, FairyRingOptions};
use crate::options::{
    AnnulusOptions, BulbOptions, CapOptions, MaterialOptions, MushroomOptions, Pattern,
    StemOptions, UndersideOptions, WartOptions,
};
use crate::profile::CapFamily;

/// A named, ready-made mushroom, loosely modelled on a real or storybook
/// species.
///
/// Every built-in preset has a function of its own, such as
/// [`MushroomPreset::fly_agaric`]. Clone its [`mushroom`](Self::mushroom)
/// options and change what you like, such as
/// [`MushroomOptions::default_seed`] for a different mushroom of the same
/// species.
#[derive(Debug, Clone, PartialEq)]
pub struct MushroomPreset {
    /// The preset's key, such as `"fly_agaric"`, or a loaded file's name.
    pub key: Cow<'static, str>,
    /// A short display name, such as `"Fly agaric"`.
    pub name: Cow<'static, str>,
    /// What the mushroom looks like, in a sentence or two.
    pub description: Cow<'static, str>,
    /// The group it is listed under, such as `"Mushrooms"`, or `"File"` for
    /// a loaded file.
    pub group: Cow<'static, str>,
    /// The mushroom's settings. They are always valid for a built-in preset.
    pub mushroom: MushroomOptions,
}

procgena_core::catalogue! {
    MushroomPreset {
        mushroom: MushroomOptions,
        check: |options| options.validate().map_err(|error| error.to_string()),
    }
    "Mushrooms" {
        fly_agaric {
            name: "Fly agaric",
            description: "Red cap with white warts, a white annulus and a bulbous base.",
        } => fly_agaric();
        porcini {
            name: "Porcini",
            description: "A bolete: a brown cushion cap with a cracked pattern on a fat, pale \
                stem.",
        } => porcini();
        chanterelle {
            name: "Chanterelle",
            description: "Egg-yolk funnel with wavy rim and ridges running down the stem.",
        } => chanterelle();
        parasol {
            name: "Parasol",
            description: "Large, flat, scaly cap with a knob, on a tall slim stem with an \
                annulus.",
        } => parasol();
        liberty_cap {
            name: "Liberty cap",
            description: "A small pointed cap on a thin, bent stem.",
        } => liberty_cap();
        shaggy_ink_cap {
            name: "Shaggy ink cap",
            description: "Tall egg-shaped white cap with shaggy scales.",
        } => shaggy_ink_cap();
        button {
            name: "Button",
            description: "A small white button mushroom.",
        } => button();
        enoki {
            name: "Enoki",
            description: "A tight tuft of tiny white caps on long, thin stems that sweep \
                outward.",
        } => enoki();
        puffball {
            name: "Puffball",
            description: "A white globe with fine bumps and almost no stem.",
        } => puffball();
        storybook {
            name: "Storybook",
            description: "A stylised red bell with round spots.",
        } => storybook();
        fairy_ring {
            name: "Fairy ring",
            description: "Small tan caps growing in a wide circle on open ground, planted \
                without a closed base.",
        } => fairy_ring();
    }
}

fn fly_agaric() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.12,
        cap: CapOptions {
            family: CapFamily::Convex,
            height: 0.8,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 1.1,
            radius: 0.08,
            base: 1.3,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.9,
            height: 0.18,
            depth: 0.6,
            ..base.bulb.clone()
        },
        annulus: AnnulusOptions {
            enabled: true,
            position: 0.78,
            length: 0.1,
            flare: 1.9,
            raggedness: 0.35,
        },
        warts: WartOptions {
            count: 70,
            size: 0.07,
            size_jitter: 0.6,
            height: 0.45,
            coverage: 0.9,
        },
        material: MaterialOptions {
            cap_centre: "#a3140f".into(),
            cap_rim: "#d8401c".into(),
            underside: "#f1ead8".into(),
            stem_top: "#f4efe2".into(),
            stem_base: "#e3dac4".into(),
            annulus: "#f4efe2".into(),
            wart: "#f6f2e6".into(),
            roughness: 0.45,
            cap_texture: Some("mushroom_top_001".into()),
            cap_texture_size: 0.1,
            ..base.material.clone()
        },
        ..base
    }
}

fn porcini() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.14,
        cap: CapOptions {
            family: CapFamily::Convex,
            height: 1.1,
            asymmetry: 0.2,
            ..base.cap.clone()
        },
        underside: UndersideOptions {
            rim_thickness: 0.12,
            sag: 0.12,
            decurrent: 0.0,
        },
        stem: StemOptions {
            height: 0.75,
            radius: 0.16,
            base: 1.5,
            collar: 0.0,
            lean: 0.03,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.4,
            height: 0.5,
            depth: 0.4,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#5a3418".into(),
            cap_rim: "#8a5a30".into(),
            underside: "#d9c98a".into(),
            stem_top: "#e8dcc0".into(),
            stem_base: "#c9b48e".into(),
            pattern: Pattern::Cracks,
            pattern_colour: "#e2d2b0".into(),
            pattern_strength: 0.12,
            pattern_scale: 6.0,
            gills: false,
            stem_texture: Some("brown_net".into()),
            stem_texture_size: 0.04,
            ..base.material.clone()
        },
        ..base
    }
}

fn chanterelle() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.07,
        cap: CapOptions {
            family: CapFamily::Infundibuliform,
            lobes: 5,
            lobe_depth: 0.12,
            asymmetry: 0.25,
            ..base.cap.clone()
        },
        underside: UndersideOptions {
            rim_thickness: 0.04,
            sag: -0.05,
            decurrent: 0.45,
        },
        stem: StemOptions {
            height: 0.7,
            radius: 0.11,
            base: 0.7,
            collar: 0.4,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.0,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#e09a1c".into(),
            cap_rim: "#f0b432".into(),
            underside: "#eaa62a".into(),
            stem_top: "#eaa62a".into(),
            stem_base: "#d89a3a".into(),
            gill_lines: 40,
            ..base.material.clone()
        },
        ..base
    }
}

fn parasol() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.22,
        cap: CapOptions {
            family: CapFamily::Umbonate,
            height: 0.8,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 1.3,
            radius: 0.04,
            base: 1.2,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 1.0,
            height: 0.1,
            depth: 0.7,
            ..base.bulb.clone()
        },
        annulus: AnnulusOptions {
            enabled: true,
            position: 0.7,
            length: 0.05,
            flare: 2.5,
            raggedness: 0.15,
        },
        warts: WartOptions {
            count: 120,
            size: 0.06,
            size_jitter: 0.5,
            height: 0.15,
            coverage: 0.85,
        },
        material: MaterialOptions {
            cap_centre: "#4a3324".into(),
            cap_rim: "#d8c9b0".into(),
            underside: "#efe6d4".into(),
            stem_top: "#cdb99a".into(),
            stem_base: "#7a5f45".into(),
            annulus: "#e8dfcc".into(),
            wart: "#6b4c34".into(),
            stem_texture: Some("creamy_tan".into()),
            stem_texture_size: 0.03,
            ..base.material.clone()
        },
        ..base
    }
}

fn liberty_cap() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.02,
        cap: CapOptions {
            family: CapFamily::Conical,
            tilt: 10.0,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 3.5,
            radius: 0.06,
            base: 1.1,
            lean: 0.15,
            curve: 2.5,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.0,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#6e4a20".into(),
            cap_rim: "#b08a50".into(),
            underside: "#6a5a48".into(),
            stem_top: "#d8c8a8".into(),
            stem_base: "#bfa882".into(),
            roughness: 0.4,
            ..base.material.clone()
        },
        ..base
    }
}

fn shaggy_ink_cap() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.06,
        cap: CapOptions {
            family: CapFamily::Ovate,
            height: 1.6,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 1.6,
            radius: 0.12,
            ..base.stem.clone()
        },
        warts: WartOptions {
            count: 90,
            size: 0.09,
            size_jitter: 0.4,
            height: 0.2,
            coverage: 0.95,
        },
        material: MaterialOptions {
            cap_centre: "#c8b49a".into(),
            cap_rim: "#efe9df".into(),
            underside: "#3a3030".into(),
            stem_top: "#f2efe8".into(),
            stem_base: "#e4dfd4".into(),
            wart: "#d9cbb6".into(),
            ..base.material.clone()
        },
        ..base
    }
}

fn button() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.045,
        cap: CapOptions {
            family: CapFamily::Convex,
            height: 1.2,
            asymmetry: 0.05,
            ..base.cap.clone()
        },
        underside: UndersideOptions {
            rim_thickness: 0.15,
            sag: 0.1,
            decurrent: 0.0,
        },
        stem: StemOptions {
            height: 0.5,
            radius: 0.17,
            lean: 0.0,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.0,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#e9e2d6".into(),
            cap_rim: "#f3eee6".into(),
            underside: "#b8907a".into(),
            stem_top: "#f1ece3".into(),
            stem_base: "#ddd4c4".into(),
            roughness: 0.6,
            ..base.material.clone()
        },
        ..base
    }
}

fn enoki() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.012,
        cap: CapOptions {
            family: CapFamily::Convex,
            asymmetry: 0.05,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 7.0,
            radius: 0.14,
            base: 1.0,
            lean: 0.1,
            curve: 2.0,
            upturn: 0.7,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.0,
            ..base.bulb.clone()
        },
        // Long stalks that sweep outward from a tight base.
        cluster: ClusterOptions {
            stems: 14,
            lean: 5.0,
            sweep: 0.35,
            ..base.cluster.clone()
        },
        material: MaterialOptions {
            cap_centre: "#e6dcc0".into(),
            cap_rim: "#f3eedc".into(),
            underside: "#efe8d6".into(),
            stem_top: "#f6f1e4".into(),
            stem_base: "#d6c7a2".into(),
            gill_lines: 24,
            ..base.material.clone()
        },
        ..base
    }
}

fn puffball() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.07,
        cap: CapOptions {
            family: CapFamily::Globe,
            height: 0.85,
            tilt: 0.0,
            roughness: 0.02,
            ..base.cap.clone()
        },
        underside: UndersideOptions {
            rim_thickness: 0.0,
            sag: 0.0,
            decurrent: 0.0,
        },
        stem: StemOptions {
            height: 0.08,
            radius: 0.12,
            base: 1.1,
            collar: 0.0,
            lean: 0.0,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.0,
            ..base.bulb.clone()
        },
        warts: WartOptions {
            count: 160,
            size: 0.035,
            size_jitter: 0.5,
            height: 0.5,
            coverage: 0.8,
        },
        material: MaterialOptions {
            cap_centre: "#f1ede4".into(),
            cap_rim: "#e2dccd".into(),
            underside: "#e2dccd".into(),
            stem_top: "#e2dccd".into(),
            stem_base: "#cfc6b2".into(),
            wart: "#f7f4ee".into(),
            gills: false,
            ..base.material.clone()
        },
        ..base
    }
}

fn storybook() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.1,
        cap: CapOptions {
            family: CapFamily::Campanulate,
            height: 0.9,
            roughness: 0.0,
            asymmetry: 0.05,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 0.9,
            radius: 0.12,
            base: 1.25,
            roughness: 0.0,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            size: 0.15,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#c8201a".into(),
            cap_rim: "#d83a24".into(),
            underside: "#f2e6cc".into(),
            stem_top: "#f6eedc".into(),
            stem_base: "#eadcbc".into(),
            pattern: Pattern::Spots,
            pattern_colour: "#fbf6ea".into(),
            pattern_scale: 4.0,
            pattern_strength: 1.0,
            roughness: 0.5,
            ..base.material.clone()
        },
        ..base
    }
}

fn fairy_ring() -> MushroomOptions {
    let base = MushroomOptions::default();
    MushroomOptions {
        size: 0.035,
        cap: CapOptions {
            family: CapFamily::Umbonate,
            height: 0.9,
            asymmetry: 0.2,
            ..base.cap.clone()
        },
        stem: StemOptions {
            height: 1.5,
            radius: 0.07,
            base: 1.1,
            lean: 0.06,
            ..base.stem.clone()
        },
        bulb: BulbOptions {
            closed: false,
            size: 0.0,
            ..base.bulb.clone()
        },
        material: MaterialOptions {
            cap_centre: "#a07040".into(),
            cap_rim: "#d2b48a".into(),
            underside: "#e6d6b6".into(),
            stem_top: "#dcc8a4".into(),
            stem_base: "#b89a70".into(),
            gill_lines: 36,
            ..base.material.clone()
        },
        cluster: ClusterOptions {
            stems: 40,
            layout: ClusterLayout::FairyRing,
            size_variation: 0.5,
            lean: 10.0,
            variants: 4,
            fairy_ring: FairyRingOptions {
                radius: 16.0,
                width: 0.06,
                arc: 0.85,
            },
            ..base.cluster.clone()
        },
        ..base
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    /// Checks that menu entries have unique keys and names and valid
    /// settings.
    #[test]
    fn presets_are_valid() {
        let mut keys = HashSet::new();
        let mut names = HashSet::new();
        for preset in MushroomPreset::all() {
            preset
                .mushroom
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", preset.name));
            assert!(keys.insert(&preset.key), "{} is listed twice", preset.key);
            assert!(
                names.insert(&preset.name),
                "{} is listed twice",
                preset.name
            );
        }
    }

    /// Checks that every preset survives a trip through a `.mushroom.yml`
    /// file, written against no base and against another preset.
    #[test]
    fn every_preset_reads_back_from_its_file() {
        for preset in MushroomPreset::all() {
            let text = preset.to_yaml(None);
            let read = MushroomPreset::from_yaml(&preset.key, &text).unwrap();
            assert_eq!(read.mushroom, preset.mushroom, "{}", preset.key);
            assert_eq!(read.name, preset.name);
            // Fly agaric has a cap texture and porcini a stem texture, so a
            // preset without one must write it as an explicit null.
            for base in [MushroomPreset::fly_agaric(), MushroomPreset::porcini()] {
                let text = preset.to_yaml(Some(base));
                let read = MushroomPreset::from_yaml(&preset.key, &text).unwrap();
                assert_eq!(
                    read.mushroom, preset.mushroom,
                    "{} on {}",
                    preset.key, base.key
                );
            }
        }
    }
}
