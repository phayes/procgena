//! Controls for one of an Ecotope's ground layers: its material, place in
//! the stack and strength, the patches that break it up, the slope and
//! height it keeps to, and how it follows a Scatter.

use procgena_core::ChildKey;
use procgena_terrain::ground::{
    Condition, GroundLayer, GroundNoise, HeightRule, Ramp, Reach, ScatterInfluence,
};

use crate::params::{Group, Param, Section, count, number};

/// Highest order the Order control reaches.
pub const MAX_ORDER: u32 = 1000;

/// The materials a layer can be made of: the keys of the standard ground
/// material catalogue, in its order.
pub const MATERIALS: [&str; 11] = [
    "bare_dirt",
    "cliff_rock",
    "forest_soil",
    "grass",
    "gravel_road",
    "leaf_litter",
    "moss",
    "mud",
    "needle_litter",
    "riverbed",
    "snow",
];

/// Reads a layer's band for one condition, if it has that condition.
type RampOf = fn(&GroundLayer) -> Option<Ramp>;
/// The layer's band for one condition, added open if it has none.
type RampMut = fn(&mut GroundLayer) -> &mut Ramp;
/// Removes one condition from a layer.
type RampRemove = fn(&mut GroundLayer);

/// Every section of a ground layer's controls.
pub fn layer_sections() -> Vec<Section<GroundLayer>> {
    vec![
        Section {
            title: "Layer",
            groups: vec![Group::plain(vec![
                material(),
                count!(GroundLayer, "order", "Order", (0, MAX_ORDER), |l| l.order),
                number!(GroundLayer, "opacity", "Opacity", (0.0, 1.0, 0.01), |l| l
                    .opacity),
            ])],
        },
        Section {
            title: "Patches",
            groups: vec![Group::plain(noise_params())],
        },
        Section {
            title: "Slope",
            groups: vec![Group::plain(ramp_params(
                "slope",
                "Slope",
                (0.0, 1.0, 0.01),
                slope_ramp,
                slope_ramp_mut,
                |l| l.when.retain(|c| !matches!(c, Condition::Slope(_))),
            ))],
        },
        Section {
            title: "Height",
            groups: vec![Group::plain(ramp_params(
                "height",
                "Height",
                (-20.0, 200.0, 0.5),
                height_ramp,
                height_ramp_mut,
                |l| l.when.retain(|c| !matches!(c, Condition::Height(_))),
            ))],
        },
        Section {
            title: "Follows Scatter",
            groups: vec![Group::plain(influence_params())],
        },
    ]
}

/// The material, chosen from [`MATERIALS`].
fn material() -> Param<GroundLayer> {
    Param::choice(
        "material",
        "Material",
        &MATERIALS,
        |l: &GroundLayer| {
            MATERIALS
                .iter()
                .position(|m| *m == l.material.as_str())
                .unwrap_or(0)
        },
        |l: &mut GroundLayer, index| {
            if let Some(key) = MATERIALS.get(index).and_then(|m| ChildKey::new(m).ok()) {
                l.material = key;
            }
        },
    )
}

/// Patches: whether noise breaks the layer up, and the noise's settings.
fn noise_params() -> Vec<Param<GroundLayer>> {
    let has = |l: &GroundLayer| l.noise.is_some();
    let read = |l: &GroundLayer| l.noise.unwrap_or_default();
    let noise = |l: &mut GroundLayer| l.noise.get_or_insert_with(GroundNoise::default);
    vec![
        Param::toggle("noise", "Patchy", has, |l: &mut GroundLayer, on| {
            l.noise = on.then(|| l.noise.unwrap_or_default());
        }),
        Param::number(
            "noise.wavelength",
            "Patch Size",
            (0.5, 50.0, 0.1),
            move |l| read(l).wavelength,
            move |l, v| noise(l).wavelength = v,
        )
        .exponential()
        .shown_if(has),
        Param::number(
            "noise.contrast",
            "Edge Sharpness",
            (0.0, 6.0, 0.05),
            move |l| read(l).contrast,
            move |l, v| noise(l).contrast = v,
        )
        .shown_if(has),
        Param::number(
            "noise.threshold",
            "Coverage Threshold",
            (0.0, 1.0, 0.01),
            move |l| read(l).threshold,
            move |l, v| noise(l).threshold = v,
        )
        .shown_if(has),
    ]
}

/// A condition's band: whether the layer has it, whether it comes in
/// (rise) and goes out (fall), and where. `key` and `label` name the
/// condition; `range` is the controls' `(min, max, step)`.
fn ramp_params(
    key: &'static str,
    label: &'static str,
    range: (f32, f32, f32),
    get: RampOf,
    get_mut: RampMut,
    remove: RampRemove,
) -> Vec<Param<GroundLayer>> {
    let (min, max, _) = range;
    let has = move |l: &GroundLayer| get(l).is_some();
    let has_rise = move |l: &GroundLayer| get(l).is_some_and(|r| r.rise.is_some());
    let has_fall = move |l: &GroundLayer| get(l).is_some_and(|r| r.fall.is_some());
    let rise = move |l: &GroundLayer| get(l).and_then(|r| r.rise).unwrap_or([min, min]);
    let fall = move |l: &GroundLayer| get(l).and_then(|r| r.fall).unwrap_or([max, max]);
    let span = max - min;
    vec![
        Param::toggle(key, format!("{label} Rule"), has, move |l, on| {
            if on {
                // A new rule comes in over the lowest fifth of the range.
                *get_mut(l) = Ramp::rise(min, min + span * 0.2);
            } else {
                remove(l);
            }
        }),
        Param::toggle(format!("{key}.rise"), "Comes In", has_rise, move |l, on| {
            get_mut(l).rise = on.then(|| rise(l));
        })
        .shown_if(has),
        Param::number(
            format!("{key}.rise.0"),
            "In From",
            range,
            move |l| rise(l)[0],
            move |l, v| get_mut(l).rise = Some([v, rise(l)[1].max(v)]),
        )
        .shown_if(has_rise),
        Param::number(
            format!("{key}.rise.1"),
            "Fully In At",
            range,
            move |l| rise(l)[1],
            move |l, v| get_mut(l).rise = Some([rise(l)[0].min(v), v]),
        )
        .shown_if(has_rise),
        Param::toggle(format!("{key}.fall"), "Goes Out", has_fall, move |l, on| {
            get_mut(l).fall = on.then(|| fall(l));
        })
        .shown_if(has),
        Param::number(
            format!("{key}.fall.0"),
            "Out From",
            range,
            move |l| fall(l)[0],
            move |l, v| get_mut(l).fall = Some([v, fall(l)[1].max(v)]),
        )
        .shown_if(has_fall),
        Param::number(
            format!("{key}.fall.1"),
            "Fully Out At",
            range,
            move |l| fall(l)[1],
            move |l, v| get_mut(l).fall = Some([fall(l)[0].min(v), v]),
        )
        .shown_if(has_fall),
    ]
}

/// The layer's slope band, if it has a slope condition.
fn slope_ramp(l: &GroundLayer) -> Option<Ramp> {
    l.when.iter().find_map(|c| match c {
        Condition::Slope(ramp) => Some(*ramp),
        _ => None,
    })
}

/// The layer's slope band, added open if it has none.
fn slope_ramp_mut(l: &mut GroundLayer) -> &mut Ramp {
    let index = match l.when.iter().position(|c| matches!(c, Condition::Slope(_))) {
        Some(index) => index,
        None => {
            l.when.push(Condition::Slope(Ramp::default()));
            l.when.len() - 1
        }
    };
    match &mut l.when[index] {
        Condition::Slope(ramp) => ramp,
        _ => unreachable!("the index is a slope condition's"),
    }
}

/// The layer's height band, if it has a height condition in metres.
fn height_ramp(l: &GroundLayer) -> Option<Ramp> {
    l.when.iter().find_map(|c| match c {
        Condition::Height(HeightRule::Absolute(ramp)) => Some(*ramp),
        _ => None,
    })
}

/// The layer's height band in metres, replacing any height condition of
/// another form, or added open if it has none.
fn height_ramp_mut(l: &mut GroundLayer) -> &mut Ramp {
    let index = match l.when.iter().position(|c| matches!(c, Condition::Height(_))) {
        Some(index) => index,
        None => {
            l.when.push(Condition::Height(HeightRule::Absolute(Ramp::default())));
            l.when.len() - 1
        }
    };
    if !matches!(l.when[index], Condition::Height(HeightRule::Absolute(_))) {
        l.when[index] = Condition::Height(HeightRule::Absolute(Ramp::default()));
    }
    match &mut l.when[index] {
        Condition::Height(HeightRule::Absolute(ramp)) => ramp,
        _ => unreachable!("the index is an absolute height condition's"),
    }
}

/// How the layer follows its first Scatter. Which Scatter is chosen with
/// its own picker.
fn influence_params() -> Vec<Param<GroundLayer>> {
    let has = |l: &GroundLayer| !l.under.is_empty();
    let first = |l: &GroundLayer| l.under.first().cloned();
    let reach = move |l: &GroundLayer| match first(l).map(|i| i.reach) {
        Some(Reach::Metres(m)) => m,
        Some(Reach::Footprint(k)) => k,
        None => 0.0,
    };
    let falloff = move |l: &GroundLayer| {
        first(l)
            .and_then(|i| i.falloff.fall)
            .unwrap_or([0.5, 1.0])
    };
    vec![
        Param::number(
            "under.reach",
            "Reach",
            (0.0, 20.0, 0.1),
            reach,
            |l: &mut GroundLayer, v| {
                if let Some(influence) = l.under.first_mut() {
                    influence.reach = match influence.reach {
                        Reach::Metres(_) => Reach::Metres(v),
                        Reach::Footprint(_) => Reach::Footprint(v),
                    };
                }
            },
        )
        .shown_if(has),
        Param::toggle(
            "under.footprint",
            "Reach in Footprints",
            |l: &GroundLayer| matches!(l.under.first().map(|i| i.reach), Some(Reach::Footprint(_))),
            |l: &mut GroundLayer, on| {
                if let Some(influence) = l.under.first_mut() {
                    let v = match influence.reach {
                        Reach::Metres(v) | Reach::Footprint(v) => v,
                    };
                    influence.reach = if on { Reach::Footprint(v) } else { Reach::Metres(v) };
                }
            },
        )
        .shown_if(has),
        Param::number(
            "under.falloff.fall.0",
            "Fade From",
            (0.0, 1.0, 0.01),
            move |l| falloff(l)[0],
            move |l, v| {
                let end = falloff(l)[1].max(v);
                set_falloff(l, [v, end]);
            },
        )
        .shown_if(has),
        Param::number(
            "under.falloff.fall.1",
            "Fade To",
            (0.0, 1.0, 0.01),
            move |l| falloff(l)[1],
            move |l, v| {
                let start = falloff(l)[0].min(v);
                set_falloff(l, [start, v]);
            },
        )
        .shown_if(has),
        Param::toggle(
            "under.invert",
            "Away From It",
            |l: &GroundLayer| l.under.first().is_some_and(|i| i.invert),
            |l: &mut GroundLayer, on| {
                if let Some(influence) = l.under.first_mut() {
                    influence.invert = on;
                }
            },
        )
        .shown_if(has),
    ]
}

/// Sets how the layer's first influence fades across its reach.
fn set_falloff(l: &mut GroundLayer, fall: [f32; 2]) {
    if let Some(influence) = l.under.first_mut() {
        influence.falloff = Ramp {
            rise: influence.falloff.rise,
            fall: Some(fall),
        };
    }
}

/// A new influence following Scatter `scatter`: 2 m round each copy,
/// fading over the outer half.
pub fn new_influence(scatter: ChildKey) -> ScatterInfluence {
    ScatterInfluence {
        scatter,
        reach: Reach::Metres(2.0),
        falloff: Ramp::fall(0.5, 1.0),
        invert: false,
    }
}

#[cfg(test)]
mod tests {
    use procgena_terrain::ground::MaterialCatalogue;

    use super::*;
    use crate::params::checks;

    #[test]
    fn the_materials_are_the_standard_catalogue() {
        let standard: Vec<String> = MaterialCatalogue::standard()
            .iter()
            .map(|(_, key, _)| key.to_string())
            .collect();
        assert_eq!(standard, MATERIALS);
    }

    #[test]
    fn layer_controls_have_unique_keys_and_keep_the_layer_valid() {
        let params: Vec<_> = layer_sections()
            .into_iter()
            .flat_map(|s| s.groups)
            .flat_map(|g| g.params)
            .collect();
        checks::keys_are_unique(&params);
        let key = |k| ChildKey::new(k).unwrap();
        let layer = GroundLayer::new(key("moss"), 300)
            .when(Condition::Slope(Ramp::rise(0.2, 0.4)))
            .with_noise(GroundNoise::default())
            .under(new_influence(key("pines")));
        for param in &params {
            for variant in checks::variants(param, &layer) {
                assert!(variant.is_valid(), "{}: {variant:?}", param.key);
            }
        }
    }

    #[test]
    fn turning_a_rule_on_and_off_adds_and_removes_its_condition() {
        let sections = layer_sections();
        let rule = sections
            .iter()
            .flat_map(|s| &s.groups)
            .flat_map(|g| &g.params)
            .find(|p| p.key == "slope")
            .unwrap();
        let crate::params::Control::Toggle { set, .. } = &rule.control else {
            panic!("the slope rule is a toggle");
        };
        let mut layer = GroundLayer::new(ChildKey::new("moss").unwrap(), 300);
        set(&mut layer, true);
        assert_eq!(slope_ramp(&layer), Some(Ramp::rise(0.0, 0.2)));
        set(&mut layer, false);
        assert!(layer.when.is_empty());
    }
}
