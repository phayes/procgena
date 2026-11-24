//! Every kind's Inspector settings, built from its metadata, against its
//! presets and options; and the Inspector's edit reports.

use bevy::ecs::system::RunSystemOnce;
use bevy::prelude::*;
use procgena_core::meta::SettingPath;
use procgena_core::preset::Preset;
use procgena_fruit::{FlowerPreset, FruitPreset};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::json;

use super::*;
use crate::params::checks::{self, variants};
use crate::params::{Param, Section};

/// Every setting of `sections`, in display order.
fn params_of<T>(sections: Vec<Section<T>>) -> Vec<Param<T>> {
    sections
        .into_iter()
        .flat_map(|section| section.groups)
        .flat_map(|group| group.params)
        .collect()
}

/// Each built-in preset of `P`, as Inspector settings named by key.
fn cases<P: Preset>() -> Vec<(String, Settings<P::Options>)>
where
    P::Options: Serialize + DeserializeOwned + Clone,
{
    P::all()
        .map(|p| (p.key().to_owned(), Settings::new(p.options().clone())))
        .collect()
}

/// Which settings each range end and choice is tried on.
enum Ends {
    /// The options' defaults, for kinds whose presets hold settings that
    /// bound each other (such as a smallest and a largest value).
    OnDefaults,
    /// Every built-in preset.
    OnPresets,
    /// Not tried: for kinds whose settings bound each other at every value,
    /// such as the clump's facing start, which must stay below its facing
    /// full.
    NotTried,
}

/// Checks `kind`'s settings against `P`'s presets: unique keys, every
/// range end and choice accepted by [`Preset::check`] (on the cases `ends`
/// names), every preset value inside its control, and every control able
/// to change the settings wherever it shows.
fn check_kind<P: Preset>(kind: &'static MetaKind, custom: &CustomGroup<P::Options>, ends: Ends)
where
    P::Options: Serialize + DeserializeOwned + Clone + Default + PartialEq + Send + Sync + 'static,
{
    let cases = cases::<P>();
    let params = params_of(sections(kind, cases[0].1.value(), custom));
    checks::keys_are_unique(&params);
    let end_cases = match ends {
        Ends::OnDefaults => vec![("default".to_owned(), Settings::new(P::Options::default()))],
        Ends::OnPresets => cases.clone(),
        Ends::NotTried => Vec::new(),
    };
    checks::range_ends_are_valid(&params, &end_cases, |s| P::check(s.options()));
    checks::cases_fit_the_controls(&params, &cases);
    for param in &params {
        let mut shown = cases
            .iter()
            .filter(|(_, case)| param.is_shown(case))
            .peekable();
        if shown.peek().is_none() {
            continue;
        }
        let changes = shown.any(|(_, case)| variants(param, case).iter().any(|v| v != case));
        assert!(
            changes,
            "{}: {} never changes the settings",
            kind.meta.kind, param.key
        );
    }
}

#[test]
fn stone_settings_fit_their_presets() {
    check_kind::<procgena_stone::StonePreset>(&crate::stones::META, &none, Ends::OnDefaults);
}

#[test]
fn mushroom_settings_fit_their_presets() {
    check_kind::<procgena_mushroom::MushroomPreset>(
        &crate::mushrooms::META,
        &none,
        Ends::OnPresets,
    );
}

#[test]
fn deadwood_settings_fit_their_presets() {
    check_kind::<procgena_deadwood::DeadwoodPreset>(&crate::deadwood::META, &none, Ends::OnPresets);
}

#[test]
fn clump_settings_fit_their_presets() {
    check_kind::<procgena_clump::ClumpPreset>(&crate::clump::META, &none, Ends::NotTried);
}

#[test]
fn fruit_and_flower_settings_fit_their_presets() {
    check_kind::<FruitPreset>(&crate::fruit::FRUIT_META, &none, Ends::OnPresets);
    check_kind::<FlowerPreset>(&crate::fruit::FLOWER_META, &none, Ends::OnPresets);
}

#[test]
fn tree_settings_fit_their_presets() {
    check_kind::<procgena_tree::TreePreset>(
        &crate::tree::META,
        &crate::tree::custom,
        Ends::OnDefaults,
    );
}

#[test]
fn tree_scatter_settings_fit_their_presets() {
    check_kind::<procgena_tree::TreeScatterPreset>(&crate::grove::META, &none, Ends::OnPresets);
}

#[test]
fn fruit_and_flower_edits_keep_the_items_kind() {
    // Each kind's settings edit only its own kind of item.
    for (kind, cases) in [
        (&*crate::fruit::FRUIT_META, cases::<FruitPreset>()),
        (&*crate::fruit::FLOWER_META, cases::<FlowerPreset>()),
    ] {
        let params = params_of(sections(kind, cases[0].1.value(), &|_, _| Vec::new()));
        for (name, case) in &cases {
            for param in &params {
                for changed in variants(param, case) {
                    assert_eq!(
                        changed.options().item.is_fruit(),
                        case.options().item.is_fruit(),
                        "{name}: {} changes the kind of item",
                        param.key
                    );
                }
            }
        }
    }
}

/// Checks that every setting of `kind`, laid out for `P`'s first preset,
/// has a tooltip and a part when the kind has parts.
fn check_tooltips_and_parts<P: Preset>(kind: &'static MetaKind, custom: &CustomGroup<P::Options>)
where
    P::Options: Serialize + DeserializeOwned + Clone + PartialEq + Send + Sync + 'static,
{
    let value = cases::<P>().remove(0).1;
    for param in params_of(sections(kind, value.value(), custom)) {
        let name = &kind.meta.kind;
        assert!(
            param.description.is_some_and(|d| !d.is_empty()),
            "{name}: {} has no tooltip",
            param.key
        );
        assert!(
            kind.meta.parts.is_empty() || param.part.is_some(),
            "{name}: {} targets no part",
            param.key
        );
    }
}

/// No custom controls.
fn none<T>(_: &str, _: &'static procgena_core::meta::GroupMeta) -> Vec<Param<Settings<T>>> {
    Vec::new()
}

#[test]
fn every_setting_has_a_tooltip_and_a_part() {
    check_tooltips_and_parts::<procgena_stone::StonePreset>(&crate::stones::META, &none);
    check_tooltips_and_parts::<procgena_mushroom::MushroomPreset>(&crate::mushrooms::META, &none);
    check_tooltips_and_parts::<procgena_deadwood::DeadwoodPreset>(&crate::deadwood::META, &none);
    check_tooltips_and_parts::<procgena_clump::ClumpPreset>(&crate::clump::META, &none);
    check_tooltips_and_parts::<FruitPreset>(&crate::fruit::FRUIT_META, &none);
    check_tooltips_and_parts::<FlowerPreset>(&crate::fruit::FLOWER_META, &none);
    check_tooltips_and_parts::<procgena_tree::TreePreset>(&crate::tree::META, &crate::tree::custom);
    check_tooltips_and_parts::<procgena_tree::TreeScatterPreset>(&crate::grove::META, &none);
}

#[test]
fn units_join_the_label_and_optional_numbers_get_a_switch() {
    let piece = procgena_core::preset::to_value(&procgena_deadwood::DeadwoodOptions::default());
    let params = params_of(sections::<procgena_deadwood::DeadwoodOptions>(
        &crate::deadwood::META,
        &piece,
        &|_, _| Vec::new(),
    ));
    let label = |key: &str| {
        params
            .iter()
            .find(|p| p.key == key)
            .map(|p| p.label.clone())
            .unwrap_or_else(|| panic!("no setting {key}"))
    };
    assert_eq!(label("length"), "Length (m)");
    assert_eq!(label("bark.stripped.set"), "Peeled: Set by Hand");
    assert_eq!(label("bark.stripped"), "Peeled");
}

#[test]
fn list_items_get_their_own_settings_without_the_list_prefix() {
    let tree = procgena_tree::TreeScatterPreset::all()
        .next()
        .unwrap()
        .tree_scatter
        .trees[0]
        .clone();
    let share = Settings::new(tree);
    let species = crate::grove::META
        .meta
        .sections
        .iter()
        .flat_map(|section| &section.groups)
        .find(|group| group.custom.as_deref() == Some("species_list"))
        .unwrap();
    let params = item_params::<procgena_tree::ScatterTree>(
        &crate::grove::META,
        species,
        &SettingPath::parse("trees.0"),
        share.value(),
    );
    let keys: Vec<&str> = params.iter().map(|p| p.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "density_scale",
            "clustering",
            "spacing",
            "variants",
            "max_slope.set",
            "max_slope"
        ]
    );
}

/// The edits reported by [`InspectorEdited`], as a resource.
#[derive(Resource, Default)]
struct Reports(Vec<InspectorEdited>);

/// An app with a stone document whose page holds a stone panel. Returns
/// the app, the document and the panel.
fn stone_document() -> (App, Entity, Entity) {
    use procgena_stone::StoneOptions;

    let mut app = App::new();
    app.add_plugins((MinimalPlugins, InspectorPlugin::<StoneOptions>::default()))
        .init_resource::<Selection>()
        .init_resource::<Reports>()
        .add_observer(
            |edited: On<InspectorEdited>, mut reports: ResMut<Reports>| {
                reports.0.push(edited.event().clone());
            },
        );
    let world = app.world_mut();
    let page = world.spawn_empty().id();
    let editor = world.spawn(ChildOf(page)).id();
    let document = world
        .spawn(Document {
            kind: "stone",
            key: "boulder".into(),
            name: "Boulder".into(),
            origin: super::super::documents::Origin::BuiltIn,
            page,
            editor,
            view: None,
            detail: Default::default(),
            opened: 0,
        })
        .id();
    let panel = world
        .spawn((
            crate::params::ParamPanel::new(Settings::new(StoneOptions::default()), Vec::new()),
            ChildOf(editor),
        ))
        .id();
    app.update();
    (app, document, panel)
}

#[test]
fn an_edit_is_reported_with_its_document_and_paths() {
    let (mut app, document, panel) = stone_document();
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut panels: Query<&mut ParamPanel<Settings<procgena_stone::StoneOptions>>>| {
                edit_setting(&mut commands, &mut panels, panel, "carve.steps", json!(5));
            },
        )
        .unwrap();
    app.update();
    let reports = &app.world().resource::<Reports>().0;
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!(report.document, document);
    assert!(report.is_final);
    let paths: Vec<String> = report.edits.iter().map(|e| e.path.to_string()).collect();
    assert_eq!(paths, ["carve.steps"]);
    assert_eq!(report.edits[0].new, json!(5));

    // Setting the same value again changes and reports nothing.
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut panels: Query<&mut ParamPanel<Settings<procgena_stone::StoneOptions>>>| {
                edit_setting(&mut commands, &mut panels, panel, "carve.steps", json!(5));
            },
        )
        .unwrap();
    app.update();
    assert_eq!(app.world().resource::<Reports>().0.len(), 1);
}

#[test]
fn several_changes_of_one_action_are_reported_together() {
    // Jiggle changes many settings at once; they form one report.
    let (mut app, _, panel) = stone_document();
    let mut options = procgena_stone::StoneOptions::default();
    options.scale *= 2.0;
    options.carve.depth *= 0.5;
    app.world_mut()
        .run_system_once(
            move |mut commands: Commands,
                  mut panels: Query<&mut ParamPanel<Settings<procgena_stone::StoneOptions>>>| {
                edit_options(&mut commands, &mut panels, panel, &options);
            },
        )
        .unwrap();
    app.update();
    let reports = &app.world().resource::<Reports>().0;
    assert_eq!(reports.len(), 1);
    let mut paths: Vec<String> = reports[0]
        .edits
        .iter()
        .map(|e| e.path.to_string())
        .collect();
    paths.sort();
    assert_eq!(paths, ["carve.depth", "scale"]);
}

/// Prints how long one slider step takes on a tree: writing the setting and
/// reading the typed options back. Run with `--ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn measure_one_tree_edit() {
    let mut settings = Settings::new(procgena_tree::TreeOptions::default());
    let path = SettingPath::parse("branch.length.1");
    let steps = 1000;
    let start = std::time::Instant::now();
    for step in 0..steps {
        settings.set(&path, json!(1.0 + f64::from(step) * 0.001));
    }
    let each = start.elapsed() / steps;
    println!("one tree edit takes {each:?}");
}

#[test]
fn settings_read_back_their_options_and_refuse_wrong_shapes() {
    let mut settings = Settings::new(procgena_stone::StoneOptions::default());
    settings.set(&SettingPath::parse("carve.steps"), json!(7));
    assert_eq!(settings.options().carve.steps, 7);
    // Text where a number belongs doesn't read as options: nothing changes.
    settings.set(&SettingPath::parse("carve.steps"), json!("many"));
    assert_eq!(settings.options().carve.steps, 7);
    assert_eq!(settings.take_edits().len(), 1);
    assert!(settings.take_edits().is_empty());
}
