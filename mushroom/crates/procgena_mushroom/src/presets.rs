//! Ready-made mushroom species to start from.

use crate::options::{
    CapOptions, MaterialOptions, MushroomOptions, Pattern, RingOptions, StemOptions,
    UndersideOptions, WartOptions,
};
use crate::profile::CapFamily;

/// A ready-made set of [`MushroomOptions`], loosely modelled on a real or
/// storybook species. Use [`options`](Self::options) and change what you
/// like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MushroomPreset {
    /// Red cap with white warts, a white ring and a bulbous base.
    FlyAgaric,
    /// A bolete: a brown cushion cap with a cracked pattern on a fat, pale
    /// stem.
    Porcini,
    /// Egg-yolk funnel with wavy rim and ridges running down the stem.
    Chanterelle,
    /// Large, flat, scaly cap with a knob, on a tall slim stem with a ring.
    Parasol,
    /// A small pointed cap on a thin, bent stem.
    LibertyCap,
    /// Tall egg-shaped white cap with shaggy scales.
    ShaggyInkCap,
    /// A small white button mushroom.
    Button,
    /// Tiny white caps on long, thin stems.
    Enoki,
    /// A white globe with fine bumps and almost no stem.
    Puffball,
    /// A stylised red bell with round spots.
    Storybook,
}

impl MushroomPreset {
    /// Every preset, in the order listed here.
    pub const ALL: [MushroomPreset; 10] = [
        MushroomPreset::FlyAgaric,
        MushroomPreset::Porcini,
        MushroomPreset::Chanterelle,
        MushroomPreset::Parasol,
        MushroomPreset::LibertyCap,
        MushroomPreset::ShaggyInkCap,
        MushroomPreset::Button,
        MushroomPreset::Enoki,
        MushroomPreset::Puffball,
        MushroomPreset::Storybook,
    ];

    /// Returns a display name, such as `"Fly agaric"`.
    pub fn name(self) -> &'static str {
        match self {
            MushroomPreset::FlyAgaric => "Fly agaric",
            MushroomPreset::Porcini => "Porcini",
            MushroomPreset::Chanterelle => "Chanterelle",
            MushroomPreset::Parasol => "Parasol",
            MushroomPreset::LibertyCap => "Liberty cap",
            MushroomPreset::ShaggyInkCap => "Shaggy ink cap",
            MushroomPreset::Button => "Button",
            MushroomPreset::Enoki => "Enoki",
            MushroomPreset::Puffball => "Puffball",
            MushroomPreset::Storybook => "Storybook",
        }
    }

    /// Returns the preset's options. They are always valid.
    pub fn options(self) -> MushroomOptions {
        let base = MushroomOptions::default();
        match self {
            MushroomPreset::FlyAgaric => MushroomOptions {
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
                    bulb: 0.9,
                    bulb_height: 0.18,
                    ..base.stem.clone()
                },
                ring: RingOptions {
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
                    ring: "#f4efe2".into(),
                    wart: "#f6f2e6".into(),
                    roughness: 0.45,
                    cap_texture: Some("mushroom_top_001".into()),
                    cap_texture_size: 0.1,
                    ..base.material.clone()
                },
                ..base
            },
            MushroomPreset::Porcini => MushroomOptions {
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
                    bulb: 0.4,
                    bulb_height: 0.5,
                    collar: 0.0,
                    lean: 0.03,
                    ..base.stem.clone()
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
                    gill_lines: 0,
                    ..base.material.clone()
                },
                ..base
            },
            MushroomPreset::Chanterelle => MushroomOptions {
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
                    bulb: 0.0,
                    collar: 0.4,
                    ..base.stem.clone()
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
            },
            MushroomPreset::Parasol => MushroomOptions {
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
                    bulb: 1.0,
                    bulb_height: 0.1,
                    ..base.stem.clone()
                },
                ring: RingOptions {
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
                    ring: "#e8dfcc".into(),
                    wart: "#6b4c34".into(),
                    ..base.material.clone()
                },
                ..base
            },
            MushroomPreset::LibertyCap => MushroomOptions {
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
                    bulb: 0.0,
                    lean: 0.15,
                    curve: 2.5,
                    ..base.stem.clone()
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
            },
            MushroomPreset::ShaggyInkCap => MushroomOptions {
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
            },
            MushroomPreset::Button => MushroomOptions {
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
                    bulb: 0.0,
                    lean: 0.0,
                    ..base.stem.clone()
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
            },
            MushroomPreset::Enoki => MushroomOptions {
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
                    bulb: 0.0,
                    lean: 0.1,
                    curve: 2.0,
                    upturn: 0.7,
                    ..base.stem.clone()
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
            },
            MushroomPreset::Puffball => MushroomOptions {
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
                    bulb: 0.0,
                    collar: 0.0,
                    lean: 0.0,
                    ..base.stem.clone()
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
                    gill_lines: 0,
                    ..base.material.clone()
                },
                ..base
            },
            MushroomPreset::Storybook => MushroomOptions {
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
                    bulb: 0.15,
                    roughness: 0.0,
                    ..base.stem.clone()
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
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_are_valid() {
        for preset in MushroomPreset::ALL {
            preset
                .options()
                .validate()
                .unwrap_or_else(|e| panic!("{}: {e}", preset.name()));
        }
    }
}
