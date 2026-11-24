//! Dead wood controls and allowed input ranges for the Deadwood tab.

use procgena_deadwood::{
    BARK_TEXTURE_SETS, DeadwoodOptions, END_TEXTURE_SETS, EndKind, LEAF_TEXTURE_SETS, MAX_CHILDREN,
    MAX_DECAY, MAX_LEVELS, MAX_LOBES, MAX_ROOTS, MAX_SPLINTERS, MAX_SPRAY_CARDS,
    MAX_SPRAYS_PER_METRE, MAX_STUBS_PER_METRE, PieceKind,
};

use crate::params::{Group, Param, Section, choice, count, number, toggle};

/// The Bark Texture choices: "None", then each of [`BARK_TEXTURE_SETS`].
const BARK_TEXTURE_CHOICES: [&str; 1 + BARK_TEXTURE_SETS.len()] = {
    let mut names = ["None"; 1 + BARK_TEXTURE_SETS.len()];
    let mut i = 0;
    while i < BARK_TEXTURE_SETS.len() {
        names[i + 1] = BARK_TEXTURE_SETS[i];
        i += 1;
    }
    names
};

/// The End Grain choices: "None", then each of [`END_TEXTURE_SETS`].
const END_TEXTURE_CHOICES: [&str; 1 + END_TEXTURE_SETS.len()] = {
    let mut names = ["None"; 1 + END_TEXTURE_SETS.len()];
    let mut i = 0;
    while i < END_TEXTURE_SETS.len() {
        names[i + 1] = END_TEXTURE_SETS[i];
        i += 1;
    }
    names
};

/// The end shapes, in menu order, each with its default settings.
const END_KINDS: [(&str, EndKind); 5] = [
    ("Saw Cut", EndKind::Saw { tilt: 0.0 }),
    (
        "Chainsaw Cut",
        EndKind::Chainsaw {
            tilt: 0.0,
            step: 0.1,
        },
    ),
    ("Axe Cut", EndKind::Axe { angle: 60.0 }),
    (
        "Snapped",
        EndKind::Snapped {
            jaggedness: 0.35,
            splinters: 6,
            one_sided: 0.4,
        },
    ),
    ("Natural Tip", EndKind::Taper {}),
];

/// The names of [`END_KINDS`], for the menu.
const END_KIND_NAMES: [&str; 5] = [
    END_KINDS[0].0,
    END_KINDS[1].0,
    END_KINDS[2].0,
    END_KINDS[3].0,
    END_KINDS[4].0,
];

/// Which entry of [`END_KINDS`] `end` is.
fn end_index(end: &EndKind) -> usize {
    match end {
        EndKind::Saw { .. } => 0,
        EndKind::Chainsaw { .. } => 1,
        EndKind::Axe { .. } => 2,
        EndKind::Snapped { .. } => 3,
        EndKind::Taper {} => 4,
    }
}

/// Reads one end of the options: the base or the tip.
type EndRef = fn(&DeadwoodOptions) -> &EndKind;
/// Changes one end of the options: the base or the tip.
type EndMut = fn(&mut DeadwoodOptions) -> &mut EndKind;

/// The controls for one end: its shape, then the settings of that shape.
/// `key` is the end's field path, such as `"ends.base"`.
fn end_params(key: &str, get: EndRef, get_mut: EndMut) -> Vec<Param<DeadwoodOptions>> {
    // One number setting of the end, shown only for the shapes that have it.
    // Other shapes read as the range's minimum.
    let setting = |field: &str,
                   label: &str,
                   range: (f32, f32, f32),
                   read: fn(&EndKind) -> Option<f32>,
                   write: fn(&mut EndKind, f32)| {
        Param::number(
            format!("{key}.{field}"),
            label,
            range,
            move |o: &DeadwoodOptions| read(get(o)).unwrap_or(range.0),
            move |o: &mut DeadwoodOptions, value| write(get_mut(o), value),
        )
        .shown_if(move |o| read(get(o)).is_some())
    };

    vec![
        Param::choice(
            format!("{key}.kind"),
            "Shape",
            &END_KIND_NAMES,
            move |o: &DeadwoodOptions| end_index(get(o)),
            move |o: &mut DeadwoodOptions, index| {
                // Keep the settings of an end that doesn't change shape.
                if let Some((_, kind)) = END_KINDS.get(index)
                    && end_index(get(o)) != index
                {
                    *get_mut(o) = *kind;
                }
            },
        ),
        setting(
            "tilt",
            "Tilt (°)",
            (0.0, 45.0, 0.5),
            |e| match e {
                EndKind::Saw { tilt } | EndKind::Chainsaw { tilt, .. } => Some(*tilt),
                _ => None,
            },
            |e, v| match e {
                EndKind::Saw { tilt } => *tilt = v,
                EndKind::Chainsaw { tilt, .. } => *tilt = v.min(30.0),
                _ => {}
            },
        ),
        setting(
            "step",
            "Step",
            (0.0, 0.3, 0.01),
            |e| match e {
                EndKind::Chainsaw { step, .. } => Some(*step),
                _ => None,
            },
            |e, v| {
                if let EndKind::Chainsaw { step, .. } = e {
                    *step = v;
                }
            },
        ),
        setting(
            "angle",
            "Wedge Angle (°)",
            (30.0, 150.0, 1.0),
            |e| match e {
                EndKind::Axe { angle } => Some(*angle),
                _ => None,
            },
            |e, v| {
                if let EndKind::Axe { angle } = e {
                    *angle = v;
                }
            },
        ),
        setting(
            "jaggedness",
            "Jaggedness",
            (0.0, 1.0, 0.01),
            |e| match e {
                EndKind::Snapped { jaggedness, .. } => Some(*jaggedness),
                _ => None,
            },
            |e, v| {
                if let EndKind::Snapped { jaggedness, .. } = e {
                    *jaggedness = v;
                }
            },
        ),
        setting(
            "splinters",
            "Splinters",
            (0.0, MAX_SPLINTERS as f32, 1.0),
            |e| match e {
                EndKind::Snapped { splinters, .. } => Some(*splinters as f32),
                _ => None,
            },
            |e, v| {
                if let EndKind::Snapped { splinters, .. } = e {
                    *splinters = v.round().max(0.0) as u32;
                }
            },
        ),
        setting(
            "one_sided",
            "Slant",
            (0.0, 1.0, 0.01),
            |e| match e {
                EndKind::Snapped { one_sided, .. } => Some(*one_sided),
                _ => None,
            },
            |e, v| {
                if let EndKind::Snapped { one_sided, .. } = e {
                    *one_sided = v;
                }
            },
        ),
    ]
}

/// Controls for a setting that normally follows decay: a "Set by Hand"
/// toggle, and the value, shown and changeable only while it is on.
/// Turning the toggle on starts from `start`.
macro_rules! optional {
    ($key:expr, $label:expr, $range:expr, $start:expr, |$o:ident| $place:expr) => {
        [
            Param::toggle(
                concat!($key, ".set"),
                concat!($label, ": Set by Hand"),
                |$o: &DeadwoodOptions| $place.is_some(),
                |$o: &mut DeadwoodOptions, on| $place = on.then_some($start),
            ),
            Param::number(
                $key,
                $label,
                $range,
                |$o: &DeadwoodOptions| $place.unwrap_or($start),
                |$o: &mut DeadwoodOptions, value| {
                    if $place.is_some() {
                        $place = Some(value);
                    }
                },
            )
            .shown_if(|$o: &DeadwoodOptions| $place.is_some()),
        ]
    };
}

/// A colour picker for a `"#rrggbb"` text field of [`DeadwoodOptions`].
macro_rules! hex_colour {
    ($key:expr, $label:expr, |$o:ident| $place:expr) => {
        Param::color(
            $key,
            $label,
            |$o: &DeadwoodOptions| hex_to_picker(&$place),
            |$o: &mut DeadwoodOptions, value| $place = picker_to_hex(value),
        )
    };
}

/// A choice between "None" and the texture sets in `$sets`, for an
/// `Option<String>` field; `$choices` is "None" followed by `$sets`.
macro_rules! texture_choice {
    ($key:expr, $label:expr, $choices:expr, $sets:expr, |$o:ident| $place:expr) => {
        Param::choice(
            $key,
            $label,
            &$choices,
            |$o: &DeadwoodOptions| {
                $place
                    .as_deref()
                    .and_then(|set| $sets.iter().position(|s| *s == set))
                    .map_or(0, |i| i + 1)
            },
            |$o: &mut DeadwoodOptions, index| {
                $place = index
                    .checked_sub(1)
                    .and_then(|i| $sets.get(i))
                    .map(|set| (*set).to_owned());
            },
        )
    };
}

/// Whether `o` is a branch, the only kind with side branches.
fn is_branch(o: &DeadwoodOptions) -> bool {
    o.kind == PieceKind::Branch
}

/// Returns the dead wood controls grouped in display order, with labels and
/// input limits. Preset and seed controls are separate.
pub fn sections() -> Vec<Section<DeadwoodOptions>> {
    // Leaf sprays: only the count shows until it is above zero.
    let has_leaves = |o: &DeadwoodOptions| o.leaves.per_metre > 0.0;
    let leaves = vec![
        number!(
            DeadwoodOptions,
            "leaves.per_metre",
            "Sprays per Metre",
            (0.0, MAX_SPRAYS_PER_METRE, 0.1),
            |o| o.leaves.per_metre
        ),
        Param::choice(
            "leaves.texture",
            "Texture",
            &LEAF_TEXTURE_SETS,
            |o: &DeadwoodOptions| {
                LEAF_TEXTURE_SETS
                    .iter()
                    .position(|s| *s == o.leaves.texture)
                    .unwrap_or(0)
            },
            |o: &mut DeadwoodOptions, index| {
                o.leaves.texture = LEAF_TEXTURE_SETS[index].to_owned();
            },
        )
        .shown_if(has_leaves),
        count!(
            DeadwoodOptions,
            "leaves.cards",
            "Cards per Spray",
            (1, MAX_SPRAY_CARDS),
            |o| o.leaves.cards
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.size",
            "Size (m)",
            (0.02, 2.0, 0.01),
            |o| o.leaves.size
        )
        .exponential()
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.size_variance",
            "Size Variation",
            (0.0, 0.9, 0.01),
            |o| o.leaves.size_variance
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.spread",
            "Spread (°)",
            (0.0, 90.0, 1.0),
            |o| o.leaves.spread
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.lean",
            "Lean (°)",
            (0.0, 90.0, 1.0),
            |o| o.leaves.lean
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.up",
            "Toward Up",
            (0.0, 1.0, 0.01),
            |o| o.leaves.up
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.flatten",
            "Flatten",
            (0.0, 1.0, 0.01),
            |o| o.leaves.flatten
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.top",
            "Prefer Top",
            (0.0, 1.0, 0.01),
            |o| o.leaves.top
        )
        .shown_if(has_leaves),
        number!(
            DeadwoodOptions,
            "leaves.ends",
            "Near Ends",
            (0.0, 1.0, 0.01),
            |o| o.leaves.ends
        )
        .shown_if(has_leaves),
        toggle!(DeadwoodOptions, "leaves.crossed", "Crossed Cards", |o| o
            .leaves
            .crossed)
        .shown_if(has_leaves),
        hex_colour!("leaves.tint", "Tint", |o| o.leaves.tint).shown_if(has_leaves),
    ];

    let mut stubs = vec![
        number!(
            DeadwoodOptions,
            "stubs.per_metre",
            "Per Metre",
            (0.0, MAX_STUBS_PER_METRE, 0.1),
            |o| o.stubs.per_metre
        ),
        number!(
            DeadwoodOptions,
            "stubs.length",
            "Length",
            (0.05, 3.0, 0.01),
            |o| o.stubs.length
        ),
        number!(
            DeadwoodOptions,
            "stubs.radius",
            "Thickness",
            (0.05, 0.8, 0.01),
            |o| o.stubs.radius
        ),
        number!(
            DeadwoodOptions,
            "stubs.angle",
            "Angle (°)",
            (10.0, 90.0, 1.0),
            |o| o.stubs.angle
        ),
    ];
    stubs.extend(optional!(
        "stubs.keep",
        "Kept",
        (0.0, 1.0, 0.01),
        1.0,
        |o| o.stubs.keep
    ));

    let mut branching = vec![
        count!(
            DeadwoodOptions,
            "branching.levels",
            "Levels",
            (0, MAX_LEVELS),
            |o| o.branching.levels
        ),
        count!(
            DeadwoodOptions,
            "branching.children",
            "Branches",
            (0, MAX_CHILDREN),
            |o| o.branching.children
        ),
        number!(
            DeadwoodOptions,
            "branching.length",
            "Length",
            (0.05, 1.0, 0.01),
            |o| o.branching.length
        ),
        number!(
            DeadwoodOptions,
            "branching.radius",
            "Thickness",
            (0.1, 0.9, 0.01),
            |o| o.branching.radius
        ),
        number!(
            DeadwoodOptions,
            "branching.angle",
            "Angle (°)",
            (5.0, 90.0, 1.0),
            |o| o.branching.angle
        ),
        count!(
            DeadwoodOptions,
            "branching.twigs",
            "Twigs",
            (0, MAX_CHILDREN),
            |o| o.branching.twigs
        ),
        number!(
            DeadwoodOptions,
            "branching.twig_length",
            "Twig Length",
            (0.02, 0.6, 0.01),
            |o| o.branching.twig_length
        ),
    ];
    branching.extend(optional!(
        "branching.keep",
        "Branches Kept",
        (0.0, 1.0, 0.01),
        1.0,
        |o| o.branching.keep
    ));
    branching.extend(optional!(
        "branching.twig_keep",
        "Twigs Kept",
        (0.0, 1.0, 0.01),
        1.0,
        |o| o.branching.twig_keep
    ));

    let mut bark = vec![number!(
        DeadwoodOptions,
        "bark.thickness",
        "Thickness",
        (0.01, 0.3, 0.005),
        |o| o.bark.thickness
    )];
    bark.extend(optional!(
        "bark.stripped",
        "Peeled",
        (0.0, 1.0, 0.01),
        0.0,
        |o| o.bark.stripped
    ));
    bark.extend([
        number!(
            DeadwoodOptions,
            "bark.patch_size",
            "Peel Patch Size (m)",
            (0.02, 2.0, 0.01),
            |o| o.bark.patch_size
        )
        .exponential(),
        number!(
            DeadwoodOptions,
            "bark.cracks",
            "Cracks",
            (0.0, 1.0, 0.01),
            |o| o.bark.cracks
        ),
        number!(
            DeadwoodOptions,
            "bark.plate_size",
            "Plate Size (m)",
            (0.01, 1.0, 0.005),
            |o| o.bark.plate_size
        )
        .exponential(),
    ]);

    let mut shape = vec![
        number!(
            DeadwoodOptions,
            "shape.bend",
            "Bend",
            (0.0, 0.5, 0.01),
            |o| o.shape.bend
        ),
        number!(
            DeadwoodOptions,
            "shape.wobble",
            "Wobble",
            (0.0, 1.0, 0.01),
            |o| o.shape.wobble
        ),
        number!(
            DeadwoodOptions,
            "shape.lumps",
            "Lumps",
            (0.0, 0.4, 0.01),
            |o| o.shape.lumps
        ),
        number!(
            DeadwoodOptions,
            "shape.lump_scale",
            "Lump Count",
            (0.5, 40.0, 0.5),
            |o| o.shape.lump_scale
        )
        .exponential(),
        number!(
            DeadwoodOptions,
            "shape.lean",
            "Lean (°)",
            (0.0, 45.0, 0.5),
            |o| o.shape.lean
        ),
    ];
    shape.extend(optional!(
        "shape.ovality",
        "Flattening",
        (0.0, 0.6, 0.01),
        0.0,
        |o| o.shape.ovality
    ));

    let mut settle = vec![toggle!(
        DeadwoodOptions,
        "settle.settle",
        "Roll to Rest",
        |o| o.settle.settle
    )];
    settle.extend(optional!("settle.sag", "Sag", (0.0, 1.0, 0.01), 0.0, |o| o
        .settle
        .sag));
    settle.extend(optional!(
        "settle.burial",
        "Burial",
        (0.0, 0.5, 0.01),
        0.0,
        |o| o.settle.burial
    ));

    vec![
        Section {
            title: "Piece",
            groups: vec![Group::plain(vec![
                choice!(DeadwoodOptions, "kind", "Kind", [
                    "Log" => PieceKind::Log,
                    "Branch" => PieceKind::Branch,
                    "Stump" => PieceKind::Stump,
                ], |o| o.kind),
                number!(
                    DeadwoodOptions,
                    "decay",
                    "Decay Class",
                    (0.0, MAX_DECAY, 0.05),
                    |o| o.decay
                ),
                number!(
                    DeadwoodOptions,
                    "length",
                    "Length (m)",
                    (0.05, 20.0, 0.01),
                    |o| o.length
                )
                .exponential(),
                number!(
                    DeadwoodOptions,
                    "diameter",
                    "Diameter (m)",
                    (0.003, 2.0, 0.001),
                    |o| o.diameter
                )
                .exponential(),
                number!(
                    DeadwoodOptions,
                    "taper",
                    "Tip Width",
                    (0.05, 1.0, 0.01),
                    |o| o.taper
                ),
            ])],
        },
        Section {
            title: "Ends",
            groups: vec![
                Group::titled(
                    "Base",
                    end_params("ends.base", |o| &o.ends.base, |o| &mut o.ends.base),
                ),
                Group::titled(
                    "Tip",
                    end_params("ends.tip", |o| &o.ends.tip, |o| &mut o.ends.tip),
                ),
            ],
        },
        Section {
            title: "Shape",
            groups: vec![Group::plain(shape)],
        },
        // Stubs always show; side branches, the hollow and roots only for
        // the kinds of piece that have them.
        Section {
            title: "Parts",
            groups: vec![
                Group::titled("Stubs", stubs),
                Group::titled("Side Branches", branching).shown_if(is_branch),
                Group::titled(
                    "Hollow",
                    vec![
                        toggle!(DeadwoodOptions, "hollow.enabled", "Hollow", |o| o
                            .hollow
                            .enabled),
                        number!(
                            DeadwoodOptions,
                            "hollow.radius",
                            "Width",
                            (0.2, 0.85, 0.01),
                            |o| o.hollow.radius
                        )
                        .shown_if(|o| o.hollow.enabled),
                        number!(
                            DeadwoodOptions,
                            "hollow.depth",
                            "Depth",
                            (0.05, 1.0, 0.01),
                            |o| o.hollow.depth
                        )
                        .shown_if(|o| o.hollow.enabled),
                        toggle!(DeadwoodOptions, "hollow.open_base", "Open at Base", |o| o
                            .hollow
                            .open_base)
                        .shown_if(|o| o.hollow.enabled),
                        toggle!(DeadwoodOptions, "hollow.open_tip", "Open at Tip", |o| o
                            .hollow
                            .open_tip)
                        .shown_if(|o| o.hollow.enabled),
                        number!(
                            DeadwoodOptions,
                            "hollow.roughness",
                            "Roughness",
                            (0.0, 0.5, 0.01),
                            |o| o.hollow.roughness
                        )
                        .shown_if(|o| o.hollow.enabled),
                    ],
                )
                .shown_if(|o| !is_branch(o)),
                Group::titled(
                    "Roots",
                    vec![
                        number!(
                            DeadwoodOptions,
                            "roots.flare",
                            "Flare",
                            (0.0, 2.0, 0.01),
                            |o| o.roots.flare
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.flare_height",
                            "Flare Height",
                            (0.05, 3.0, 0.01),
                            |o| o.roots.flare_height
                        ),
                        count!(
                            DeadwoodOptions,
                            "roots.lobes",
                            "Lobes",
                            (0, MAX_LOBES),
                            |o| o.roots.lobes
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.lobe_depth",
                            "Lobe Depth",
                            (0.0, 1.0, 0.01),
                            |o| o.roots.lobe_depth
                        ),
                        count!(
                            DeadwoodOptions,
                            "roots.count",
                            "Roots",
                            (0, MAX_ROOTS),
                            |o| o.roots.count
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.length",
                            "Root Length",
                            (0.1, 6.0, 0.01),
                            |o| o.roots.length
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.radius",
                            "Root Thickness",
                            (0.05, 0.6, 0.01),
                            |o| o.roots.radius
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.pitch",
                            "Root Dip (°)",
                            (0.0, 60.0, 0.5),
                            |o| o.roots.pitch
                        ),
                        number!(
                            DeadwoodOptions,
                            "roots.depth",
                            "Base Depth",
                            (0.05, 2.0, 0.01),
                            |o| o.roots.depth
                        ),
                    ],
                )
                .shown_if(|o| o.kind == PieceKind::Stump),
                Group::titled("Leaves", leaves),
            ],
        },
        Section {
            title: "Bark",
            groups: vec![Group::plain(bark)],
        },
        Section {
            title: "Colours",
            groups: vec![Group::plain(vec![
                hex_colour!("material.bark", "Bark", |o| o.material.bark),
                hex_colour!("material.bark_weathered", "Weathered Bark", |o| o
                    .material
                    .bark_weathered),
                hex_colour!("material.sapwood", "Sapwood", |o| o.material.sapwood),
                hex_colour!("material.heartwood", "Heartwood", |o| o.material.heartwood),
                hex_colour!("material.rot", "Rot", |o| o.material.rot),
                number!(
                    DeadwoodOptions,
                    "material.roughness",
                    "Roughness",
                    (0.0, 1.0, 0.01),
                    |o| o.material.roughness
                ),
            ])],
        },
        Section {
            title: "Textures",
            groups: vec![Group::plain(vec![
                texture_choice!(
                    "material.bark_texture",
                    "Bark",
                    BARK_TEXTURE_CHOICES,
                    BARK_TEXTURE_SETS,
                    |o| o.material.bark_texture
                ),
                number!(
                    DeadwoodOptions,
                    "material.bark_texture_size",
                    "Bark Size (m)",
                    (0.05, 3.0, 0.01),
                    |o| o.material.bark_texture_size
                )
                .exponential()
                .shown_if(|o| o.material.bark_texture.is_some()),
                texture_choice!(
                    "material.end_texture",
                    "End Grain",
                    END_TEXTURE_CHOICES,
                    END_TEXTURE_SETS,
                    |o| o.material.end_texture
                ),
            ])],
        },
        Section {
            title: "Resting",
            groups: vec![Group::plain(settle)],
        },
    ]
}

/// Reads a `"#rrggbb"` colour as the colour picker's `0xRRGGBB`; an
/// unreadable colour shows as black.
fn hex_to_picker(text: &str) -> u32 {
    u32::from_str_radix(text.trim_start_matches('#'), 16).unwrap_or(0) & 0x00ff_ffff
}

/// Writes the colour picker's `0xRRGGBB` as `"#rrggbb"`.
fn picker_to_hex(value: u32) -> String {
    format!("#{:06x}", value & 0x00ff_ffff)
}

#[cfg(test)]
mod tests {
    use procgena_deadwood::DeadwoodPreset;

    use super::*;
    use crate::params::{Control, checks};

    fn all_params() -> Vec<Param<DeadwoodOptions>> {
        sections()
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.params)
            .collect()
    }

    fn presets() -> Vec<(String, DeadwoodOptions)> {
        DeadwoodPreset::all()
            .map(|p| (p.name.to_string(), p.deadwood.clone()))
            .collect()
    }

    fn accepted(options: DeadwoodOptions) -> Result<(), String> {
        options.validate().map_err(|e| e.to_string())
    }

    #[test]
    fn texture_choices_follow_the_texture_sets() {
        assert_eq!(BARK_TEXTURE_CHOICES[1..], BARK_TEXTURE_SETS);
        assert_eq!(END_TEXTURE_CHOICES[1..], END_TEXTURE_SETS);
    }

    #[test]
    fn colours_survive_the_picker() {
        for text in ["#000000", "#8a6f52", "#ffffff"] {
            assert_eq!(picker_to_hex(hex_to_picker(text)), text);
        }
    }

    #[test]
    fn keys_are_unique() {
        checks::keys_are_unique(&all_params());
    }

    #[test]
    fn every_preset_fits_the_controls() {
        checks::cases_fit_the_controls(&all_params(), &presets());
    }

    #[test]
    fn every_control_value_is_accepted() {
        checks::range_ends_are_valid(&all_params(), &presets(), accepted);
    }

    #[test]
    fn controls_show_and_hide_sensibly() {
        checks::settings_follow_what_shows_them(&all_params(), &presets());
        checks::no_setting_hides_itself(&all_params(), &presets());
    }

    #[test]
    fn every_end_shape_can_be_chosen() {
        let params = all_params();
        let shape = params.iter().find(|p| p.key == "ends.tip.kind").unwrap();
        let Control::Choice { set, .. } = &shape.control else {
            panic!("the end shape is not a choice");
        };
        let mut options = DeadwoodOptions::default();
        for (index, (_, kind)) in END_KINDS.iter().enumerate() {
            set(&mut options, index);
            assert_eq!(end_index(&options.ends.tip), end_index(kind));
        }
    }
}
