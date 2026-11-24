use serde_json::{Value, json};

use super::*;

const FILE: &str = r#"
kind: thing
name: Thing
description: A test thing.
parts:
  body: { name: Body, tags: [body] }
  arms: { name: Arms }
conditions:
  grows_0: "true_flag"
  grows_1: "levels >= 1 && (deciduous || children.0 > 0)"
  grows_2: "@grows_1 && levels >= 2 && (deciduous || children.1 > 0)"
sections:
  - title: Body
    part: body
    settings: [size, tint, shape, levels, deciduous]
  - title: Arms
    part: arms
    groups:
      - title: Lengths
        settings: [length.*]
      - settings: [children.*]
settings:
  true_flag: { label: Flag, hidden: true }
  size: { label: Size, description: How big., range: [0.1, 10], step: 0.1, scale: exponential, unit: m }
  tint: { label: Tint, description: Its colour., color: true }
  shape:
    label: Shape
    description: Its outline.
    choices:
      - { value: { kind: round, radius: 1.0 }, label: Round }
      - { value: { kind: square, side: 2.0 }, label: Square }
  levels: { label: Levels, description: How many levels., range: [0, 2] }
  deciduous: { label: Deciduous, description: Drops its leaves. }
  length.*:
    label: Length
    description: The level's length.
    range: [0, 5]
    step: 0.01
    shown_if: "@grows_{index}"
    revive: { from: "length.{parent}", times: 0.5, min: 0.2 }
  children.*: { label: Children, description: Side arms., range: [0, 10], step: 1, shown_if: "levels > {index}", revive: 4 }
"#;

fn meta() -> KindMeta {
    KindMeta::parse(FILE).unwrap()
}

fn sample() -> Value {
    json!({
        "true_flag": true,
        "size": 1.0,
        "tint": 0x336699,
        "shape": { "kind": "round", "radius": 1.0 },
        "levels": 0,
        "deciduous": false,
        "length": { "0": 2.0, "1": 0.0, "2": 0.0 },
        "children": { "0": 0, "1": 0 },
    })
}

fn path(text: &str) -> SettingPath {
    SettingPath::parse(text)
}

#[test]
fn paths_get_and_set_inside_maps_and_lists() {
    let mut value = json!({ "a": { "b": [1, 2, { "c": true }] } });
    assert_eq!(path("a.b.2.c").get(&value), Some(&json!(true)));
    assert_eq!(path("a.b.1").set(&mut value, json!(5)), Some(json!(2)));
    assert_eq!(path("a.new").set(&mut value, json!(1)), Some(Value::Null));
    assert_eq!(path("a.b.9").set(&mut value, json!(1)), None);
    assert_eq!(path("x.y").set(&mut value, json!(1)), None);
    assert_eq!(value["a"]["b"][1], json!(5));
    assert_eq!(path("a.b.2.c").to_string(), "a.b.2.c");
}

#[test]
fn edits_record_the_old_value_and_skip_no_change() {
    let mut value = sample();
    let edit = Edit::apply(&mut value, path("size"), json!(2.0)).unwrap();
    assert_eq!((edit.old, edit.new), (json!(1.0), json!(2.0)));
    assert!(Edit::apply(&mut value, path("size"), json!(2.0)).is_none());
}

#[test]
fn patterns_match_one_segment_and_give_the_index() {
    let pattern = Pattern::parse("length.*");
    assert_eq!(pattern.matches(&path("length.2")).unwrap().index, Some(2));
    assert!(pattern.matches(&path("length")).is_none());
    assert!(pattern.matches(&path("length.2.x")).is_none());
    assert_eq!(pattern.expand(&sample()).len(), 3);
}

#[test]
fn conditions_follow_the_language() {
    struct None_;
    impl NamedConditions for None_ {
        fn named(&self, _: &str) -> Option<&Condition> {
            None
        }
    }
    let value = json!({ "n": 3, "kind": "pine", "on": true, "off": false, "list": [0, 7] });
    let holds = |text: &str| {
        Condition::parse(text)
            .unwrap()
            .holds(&value, Some(1), &None_)
    };
    assert!(holds("on"));
    assert!(!holds("off"));
    assert!(!holds("missing"));
    assert!(holds("!off && on"));
    assert!(holds("off || n > 2"));
    assert!(holds("n >= 3 && n <= 3 && n != 4 && n == 3"));
    assert!(holds("kind == pine && kind in [oak, pine]"));
    assert!(holds("kind == \"pine\""));
    assert!(holds("list.{index} == 7 && list.{parent} == 0"));
    assert!(holds("(off || on) && !(n < 1)"));
    assert!(holds("n > {index} && n in [{parent}, 3]"));
    assert!(Condition::parse("n > {index}").unwrap().uses_index());
    assert!(!Condition::parse("n > 1").unwrap().uses_index());
    assert!(!holds("@unknown"));
    for bad in ["", "n >", "(on", "n in [1", "a ==", "x.{size}", "on on"] {
        assert!(Condition::parse(bad).is_err(), "{bad:?} should not read");
    }
}

#[test]
fn files_read_and_name_their_mistakes() {
    let meta = meta();
    assert_eq!(meta.kind, "thing");
    assert_eq!(meta.parts.len(), 2);
    let (size, _) = meta.setting(&path("size")).unwrap();
    assert_eq!(
        (size.range, size.scale),
        (Some([0.1, 10.0]), Scale::Exponential)
    );
    let error = KindMeta::parse("kind: x\nname: X\nsettings:\n  a: { label: A, rnage: [0, 1] }\n")
        .unwrap_err();
    assert!(error.message.contains("settings.a"), "{error}");
    let error =
        KindMeta::parse("kind: x\nname: X\nsettings:\n  a: { label: A, shown_if: \"b >\" }\n")
            .unwrap_err();
    assert!(error.message.contains("settings.a.shown_if"), "{error}");
}

#[test]
fn named_conditions_chain_through_the_levels() {
    let meta = meta();
    let mut value = sample();
    let shown = |value: &Value, p: &str| meta.is_shown(&path(p), value);
    assert!(shown(&value, "length.0"));
    assert!(!shown(&value, "length.1"));
    assert!(
        !shown(&value, "true_flag"),
        "hidden settings are never shown"
    );
    value["levels"] = json!(2);
    value["children"]["0"] = json!(3);
    assert!(shown(&value, "length.1"));
    assert!(!shown(&value, "length.2"), "level 1 has no children");
    value["deciduous"] = json!(true);
    assert!(shown(&value, "length.2"));
}

#[test]
fn layout_expands_patterns_in_order_with_parts() {
    let meta = meta();
    let layout = meta.layout(&sample());
    let arms = &layout[1];
    let lengths: Vec<String> = arms.groups[0]
        .settings
        .iter()
        .map(|s| s.path.to_string())
        .collect();
    assert_eq!(lengths, ["length.0", "length.1", "length.2"]);
    assert_eq!(arms.groups[0].settings[0].part, Some("arms"));
    assert_eq!(meta.part_of(&path("size")), Some("body"));
}

#[test]
fn revealed_numbers_are_revived_in_turn_from_their_parent() {
    let meta = meta();
    let before = sample();
    let mut after = before.clone();
    after["levels"] = json!(1);
    assert!(meta.revive(&before, &mut after));
    // Children 0 appears and revives to 4 (kept whole), which shows level 1,
    // whose length then revives from level 0's.
    assert_eq!(after["children"]["0"], json!(4));
    assert_eq!(after["length"]["1"], json!(1.0));
    // A setting that was already shown keeps its zero.
    let before = after.clone();
    let mut again = after.clone();
    again["size"] = json!(2.0);
    assert!(!meta.revive(&before, &mut again));
}

#[test]
fn choices_match_by_kind_and_keep_that_kinds_settings() {
    let meta = meta();
    let (shape, _) = meta.setting(&path("shape")).unwrap();
    let lists = ChoiceLists::new();
    let choices = shape.choices(&lists).unwrap();
    let edited = json!({ "kind": "round", "radius": 3.0 });
    assert_eq!(choice_index(choices, &edited), Some(0));
    assert_eq!(pick_choice(&choices[0], &edited), edited);
    assert_eq!(pick_choice(&choices[1], &edited), choices[1].value);
}

#[test]
fn numbers_keep_their_kind_and_the_shortest_f32_form() {
    let meta = meta();
    let entry = |p: &str| meta.setting(&path(p)).unwrap().0;
    // Whole-number settings stay whole.
    assert_eq!(entry("children.0").number_value(&json!(3), 2.6), json!(3));
    // A fractional setting written as a whole number in a file is not
    // stuck on whole numbers.
    assert_eq!(entry("size").number_value(&json!(2), 2.6), json!(2.6));
    // A slider's f32 is stored as preset::to_value writes it.
    let slider = f64::from(0.6_f32);
    assert_eq!(entry("size").number_value(&json!(1.0), slider), json!(0.6));
    let written = crate::preset::to_value(&0.6_f32);
    assert_eq!(entry("size").number_value(&written, slider), written);
}

#[test]
fn listed_settings_stay_in_the_layout_while_absent_but_are_not_shown() {
    let meta = meta();
    let mut value = sample();
    value.as_object_mut().unwrap().remove("tint");
    let body = &meta.layout(&value)[0].groups[0].settings;
    let tint = body.iter().find(|s| s.path == path("tint")).unwrap();
    assert!(!tint.shown);
    assert!(!meta.is_shown(&path("tint"), &value));
}

const DECIDUOUS: &str = "  deciduous: { label: Deciduous, description: Drops its leaves. }";
const LEVELS: &str = "  levels: { label: Levels, description: How many levels., range: [0, 2] }";

/// `text` with each `(from, to)` replaced, checking that each `from` is there.
fn edit(text: &str, changes: &[(&str, &str)]) -> String {
    changes.iter().fold(text.to_owned(), |text, (from, to)| {
        assert!(text.contains(from), "{from:?} is not in the file");
        text.replace(from, to)
    })
}

#[test]
fn the_drift_check_passes_for_a_file_in_step() {
    assert_eq!(
        problems(
            &meta(),
            &[("default".into(), sample())],
            &ChoiceLists::new(),
            &[]
        ),
        Vec::<String>::new()
    );
}

#[test]
fn the_drift_check_names_every_mistake() {
    let text = edit(
        FILE,
        &[
            (
                DECIDUOUS,
                &format!(
                    "{DECIDUOUS}\n  ghost: {{ label: Ghost, description: G. }}\n  pet: {{ label: Pet, ref: animal }}"
                ),
            ),
            (
                LEVELS,
                &LEVELS.replace(" }", ", shown_if: \"@nope && height\" }"),
            ),
            (
                "  arms: { name: Arms }",
                "  arms: { name: Arms }\n  legs: { name: Legs }",
            ),
        ],
    );
    let meta = KindMeta::parse(&text).unwrap();
    let mut value = sample();
    value["extra"] = json!(1);
    value["pet"] = json!("cat");
    value["size"] = json!(50.0);
    let found = problems(
        &meta,
        &[("default".into(), value)],
        &ChoiceLists::new(),
        &["thing"],
    );
    let expect = [
        "extra has no entry",
        "entry ghost matches no setting",
        "entry pet is in no section",
        "entry pet refers to unknown kind animal",
        "entry pet has no description",
        "levels: a condition names unknown condition @nope",
        "levels: a condition reads height, which no sample has",
        "part legs has no setting",
        "default: size = 50 is outside 0.1..=10",
    ];
    for message in expect {
        assert!(
            found.iter().any(|f| f == message),
            "missing {message:?} in {found:#?}"
        );
    }
    assert_eq!(found.len(), expect.len(), "{found:#?}");
}

#[test]
fn choice_variants_are_tried_as_samples() {
    // The square shape's `side` appears only through the choice's option.
    let text = edit(
        FILE,
        &[(
            DECIDUOUS,
            &format!("{DECIDUOUS}\n  shape.side: {{ label: Side, hidden: true }}"),
        )],
    );
    let meta = KindMeta::parse(&text).unwrap();
    let found = problems(
        &meta,
        &[("default".into(), sample())],
        &ChoiceLists::new(),
        &[],
    );
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn variants_reach_each_end_and_option() {
    let meta = meta();
    let lists = ChoiceLists::new();
    let value = sample();
    let ends = variants(&meta, &lists, &path("levels"), &value);
    assert_eq!(
        ends.iter().map(|v| v["levels"].clone()).collect::<Vec<_>>(),
        [json!(0), json!(2)]
    );
    assert_eq!(variants(&meta, &lists, &path("shape"), &value).len(), 2);
    assert_eq!(
        variants(&meta, &lists, &path("deciduous"), &value)[0]["deciduous"],
        json!(true)
    );
    assert_eq!(variants(&meta, &lists, &path("tint"), &value).len(), 2);
}
