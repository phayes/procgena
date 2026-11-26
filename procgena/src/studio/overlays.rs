//! The Overlay control on the view frame's bar, and the overlay's legend
//! over the 3D view: what each colour means, what sampling costs, and the
//! value under the pointer.
//!
//! The control is shown only while the shown subject offers overlays (see
//! [`OverlayStatus`]). Scripts switch overlays with the `overlay-off` and
//! `overlay-<label>` commands, such as `overlay-slope` or
//! `overlay-landform`.

use bevy::feathers::controls::{FeathersSelect, OptionIndex, list_rows_from_strings};
use bevy::feathers::display::{label_dim, label_small};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::ValueChange;

use super::commands::{StudioCommand, StudioCommands};
use crate::scene::{Legend, Overlay, OverlayStatus, ViewerCommand};

/// Background behind the legend, matching the status overlay.
const LEGEND_BG: Color = Color::srgba(0.1, 0.1, 0.12, 0.85);

/// The overlay chosen on the view frame. It is never saved, and stays
/// chosen across documents; subjects that don't offer it draw none.
#[derive(Resource, Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct OverlayChoice(pub Option<Overlay>);

/// Marks where the Overlay control goes.
#[derive(Component, Default, Clone)]
struct OverlaySlot;

/// Marks the Overlay control's select.
#[derive(Component, Default, Clone)]
struct OverlaySelect;

/// Marks the legend panel.
#[derive(Component, Default, Clone)]
struct OverlayLegend;

/// Marks the legend's title.
#[derive(Component, Default, Clone)]
struct LegendTitle;

/// Marks the row of the legend's colours.
#[derive(Component, Default, Clone)]
struct LegendColours;

/// Marks the legend's cost and readout text.
#[derive(Component, Default, Clone)]
struct LegendInfo;

/// Adds the Overlay control, the legend and the overlay commands. Add it
/// after the commands plugin.
pub struct OverlaysPlugin;

impl Plugin for OverlaysPlugin {
    fn build(&self, app: &mut App) {
        let mut commands = app.world_mut().resource_mut::<StudioCommands>();
        commands.register(StudioCommand {
            name: "overlay-off",
            description: "Draws no overlay over the ground.",
            run: |world| world.resource_mut::<OverlayChoice>().0 = None,
        });
        for command in OVERLAY_COMMANDS {
            commands.register(command);
        }
        app.init_resource::<OverlayChoice>().add_systems(
            Update,
            (send_choice, show_overlay_choices, show_legend).chain(),
        );
    }
}

/// One `overlay-<label>` command per overlay.
const OVERLAY_COMMANDS: [StudioCommand; Overlay::ALL.len()] = [
    overlay_command::<0>(),
    overlay_command::<1>(),
    overlay_command::<2>(),
    overlay_command::<3>(),
    overlay_command::<4>(),
    overlay_command::<5>(),
    overlay_command::<6>(),
];

/// The command choosing the `I`th of [`Overlay::ALL`].
const fn overlay_command<const I: usize>() -> StudioCommand {
    StudioCommand {
        name: OVERLAY_NAMES[I],
        description: "Draws this overlay over the ground, where the shown subject offers it.",
        run: |world| world.resource_mut::<OverlayChoice>().0 = Some(Overlay::ALL[I]),
    }
}

/// The command names, in [`Overlay::ALL`] order: `overlay-` and the
/// overlay's label in lower case, words joined by hyphens.
const OVERLAY_NAMES: [&str; Overlay::ALL.len()] = [
    "overlay-regions",
    "overlay-landform",
    "overlay-slope",
    "overlay-curvature",
    "overlay-height-above-water",
    "overlay-water-distance",
    "overlay-road-distance",
];

/// The Overlay control, for the view frame's bar. Hidden until the shown
/// subject offers overlays.
pub fn overlay_picker() -> impl Scene {
    bsn! {
        Node {
            display: Display::None,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(8),
        }
        OverlaySlot
        Children [ @label_dim("Overlay") ]
    }
}

/// The overlay's legend, over the bottom-left corner of the 3D view.
/// Hidden while no overlay is drawn.
pub fn overlay_legend() -> impl Scene {
    bsn! {
        Node {
            position_type: PositionType::Absolute,
            bottom: px(8),
            left: px(8),
            max_width: px(360),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            row_gap: px(4),
            padding: UiRect::axes(px(8), px(6)),
            border_radius: BorderRadius::all(px(4)),
        }
        BackgroundColor(LEGEND_BG)
        Pickable::IGNORE
        OverlayLegend
        Children [
            @label_small("") LegendTitle
            --
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
            }
            LegendColours
            --
            @label_small("") LegendInfo
        ]
    }
}

/// Sends the chosen overlay to the scene whenever it changes.
fn send_choice(choice: Res<OverlayChoice>, mut viewer: MessageWriter<ViewerCommand>) {
    if choice.is_changed() {
        viewer.write(ViewerCommand::SetOverlay(choice.0));
    }
}

/// Offers the overlays the shown subject offers, with the chosen one
/// picked, and hides the control when it offers none.
fn show_overlay_choices(
    status: Res<OverlayStatus>,
    choice: Res<OverlayChoice>,
    slot: Single<(Entity, &mut Node), With<OverlaySlot>>,
    selects: Query<Entity, With<OverlaySelect>>,
    mut shown: Local<Option<(Vec<Overlay>, Option<Overlay>)>>,
    mut commands: Commands,
) {
    let now = (status.offered.clone(), choice.0);
    if shown.as_ref() == Some(&now) {
        return;
    }
    *shown = Some(now);
    for select in &selects {
        commands.entity(select).despawn();
    }
    let (slot, mut node) = slot.into_inner();
    node.display = if status.offered.is_empty() {
        Display::None
    } else {
        Display::Flex
    };
    if status.offered.is_empty() {
        return;
    }

    // A new select, since a select's choices are fixed when it is made.
    let choices: Vec<&str> = std::iter::once("Off")
        .chain(status.offered.iter().map(|o| o.label()))
        .collect();
    let picked = choice
        .0
        .and_then(|chosen| status.offered.iter().position(|&o| o == chosen))
        .map_or(0, |i| i + 1);
    commands
        .spawn_scene(bsn! {
            @FeathersSelect { @options: list_rows_from_strings(choices, Some(picked)) }
            Node { width: px(190) }
            OverlaySelect
            on(choose_overlay)
        })
        .insert(ChildOf(slot));
}

/// Keeps the overlay picked in the control.
fn choose_overlay(
    change: On<ValueChange<Entity>>,
    rows: Query<&OptionIndex>,
    status: Res<OverlayStatus>,
    mut choice: ResMut<OverlayChoice>,
) {
    let Ok(index) = rows.get(change.value) else {
        return;
    };
    let picked = index
        .0
        .checked_sub(1)
        .and_then(|i| status.offered.get(i).copied());
    choice.set_if_neq(OverlayChoice(picked));
}

/// Shows the drawn overlay's legend, cost and readout, and hides the
/// legend while none is drawn.
fn show_legend(
    status: Res<OverlayStatus>,
    legend: Single<&mut Node, With<OverlayLegend>>,
    mut title: Single<&mut Text, (With<LegendTitle>, Without<LegendInfo>)>,
    mut info: Single<&mut Text, (With<LegendInfo>, Without<LegendTitle>)>,
    colours: Single<Entity, With<LegendColours>>,
    mut drawn: Local<Option<Overlay>>,
    mut commands: Commands,
) {
    if !status.is_changed() {
        return;
    }
    let mut node = legend.into_inner();
    let Some(overlay) = status.drawn else {
        node.display = Display::None;
        *drawn = None;
        return;
    };
    node.display = Display::Flex;
    title.0 = overlay.label().to_string();
    info.0 = info_text(&status, overlay);

    // The colours change only with the overlay.
    if *drawn == Some(overlay) {
        return;
    }
    *drawn = Some(overlay);
    let colours = *colours;
    commands.entity(colours).despawn_children();
    match overlay.legend() {
        Legend::Swatches(swatches) => {
            for (colour, text) in swatches {
                swatch_row(&mut commands, colours, colour, text);
            }
        }
        Legend::Ramp {
            stops,
            low,
            high,
            extra,
        } => {
            let ramp = commands.spawn((row_node(6.0), ChildOf(colours))).id();
            commands.spawn_scene(label_small(low)).insert(ChildOf(ramp));
            let bar = commands.spawn((row_node(0.0), ChildOf(ramp))).id();
            for colour in stops {
                commands.spawn((swatch(colour, 18.0), ChildOf(bar)));
            }
            commands
                .spawn_scene(label_small(high))
                .insert(ChildOf(ramp));
            for (colour, text) in extra {
                swatch_row(&mut commands, colours, colour, text);
            }
        }
    }
}

/// One swatch of `colour` with `text` beside it, under `parent`.
fn swatch_row(commands: &mut Commands, parent: Entity, colour: [u8; 3], text: String) {
    let row = commands.spawn((row_node(6.0), ChildOf(parent))).id();
    commands.spawn((swatch(colour, 10.0), ChildOf(row)));
    commands.spawn_scene(label_small(text)).insert(ChildOf(row));
}

/// A row of the legend, its items centred and `gap` pixels apart.
fn row_node(gap: f32) -> Node {
    Node {
        display: Display::Flex,
        flex_direction: FlexDirection::Row,
        align_items: AlignItems::Center,
        column_gap: px(gap),
        ..default()
    }
}

/// A block of `colour`, `width` pixels wide and 10 high.
fn swatch(colour: [u8; 3], width: f32) -> (Node, BackgroundColor) {
    (
        Node {
            width: px(width),
            height: px(10),
            ..default()
        },
        BackgroundColor(Color::srgb_u8(colour[0], colour[1], colour[2])),
    )
}

/// The legend's text under the colours: what sampling `overlay` costs per
/// piece of ground, and the value under the pointer.
pub fn info_text(status: &OverlayStatus, overlay: Overlay) -> String {
    let family = overlay.family();
    let texels = family.texels();
    let mut lines = Vec::new();
    match status.cost.get(&family) {
        Some(cost) if cost.areas > 0 => lines.push(format!(
            "Cost: {:.1} ms per chunk ({} sampled), {texels}² texels, {}",
            cost.mean().as_secs_f64() * 1000.0,
            cost.areas,
            family.cost(),
        )),
        _ => lines.push(format!(
            "Cost: {texels}² texels per chunk, {}",
            family.cost()
        )),
    }
    if status.pending > 0 {
        lines.push(format!("Sampling {} chunks…", status.pending));
    }
    match &status.readout {
        Some(readout) => lines.push(format!(
            "Under the pointer: {} at ({:.0}, {:.0})",
            readout.text, readout.point.x, readout.point.y
        )),
        None => lines.push("Point at the ground to read it".to_string()),
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::headless::{DEFAULT_SIZE, HeadlessStudio};
    use crate::scene::SceneViewport;
    use crate::studio::status::BuildState;

    /// The command that draws `overlay`: `overlay-` and its label in lower
    /// case, words joined by hyphens.
    fn command_for(overlay: Overlay) -> String {
        format!(
            "overlay-{}",
            overlay.label().to_lowercase().replace(' ', "-")
        )
    }

    #[test]
    fn every_overlay_has_its_own_command() {
        for (overlay, name) in Overlay::ALL.iter().zip(OVERLAY_NAMES) {
            assert_eq!(name, command_for(*overlay));
        }
    }

    /// Runs `studio` until `done` holds of the overlay's status, for at
    /// most two minutes.
    fn wait_for(studio: &mut HeadlessStudio, done: impl Fn(&OverlayStatus) -> bool) {
        let start = Instant::now();
        while !done(studio.app_mut().world().resource::<OverlayStatus>()) {
            assert!(
                start.elapsed() < Duration::from_secs(120),
                "the overlay didn't settle"
            );
            studio.wait(Duration::from_millis(250));
        }
    }

    /// Runs `studio` until the terrain is drawn and `overlay` is drawn over
    /// all of it.
    fn draw(studio: &mut HeadlessStudio, overlay: Overlay) {
        studio.command(&command_for(overlay)).unwrap();
        let start = Instant::now();
        loop {
            studio.wait(Duration::from_millis(250));
            let world = studio.app_mut().world();
            let settled = world
                .resource::<BuildState>()
                .terrain
                .is_some_and(|progress| progress.settled);
            let status = world.resource::<OverlayStatus>();
            if settled && status.drawn == Some(overlay) && status.pending == 0 {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(600),
                "the terrain didn't settle"
            );
        }
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn scripts_switch_overlays_and_the_pointer_reads_the_ground() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.select("terrain/rolling_hills").unwrap();
        wait_for(&mut studio, |status| status.offered == Overlay::ALL);
        assert!(studio.find("Overlay").is_ok(), "the control is shown");

        // Picking from the control chooses the overlay.
        studio.click("Off").unwrap();
        studio.click("Curvature").unwrap();
        assert_eq!(
            studio.app_mut().world().resource::<OverlayChoice>().0,
            Some(Overlay::Curvature)
        );
        wait_for(&mut studio, |status| {
            status.drawn == Some(Overlay::Curvature)
        });

        // Slope, with its legend and cost.
        draw(&mut studio, Overlay::Slope);
        let titled = studio.texts().iter().filter(|t| t.text == "Slope").count();
        assert_eq!(titled, 2, "the picker and the legend's title name it");
        assert!(studio.shows_text_starting("Cost: "));

        // Pointing at the middle of the view reads the slope there.
        let world = studio.app_mut().world_mut();
        let viewport = world
            .query_filtered::<Entity, With<SceneViewport>>()
            .single(world)
            .unwrap();
        let middle = studio.center_of(viewport).unwrap();
        studio.hover(middle);
        wait_for(&mut studio, |status| status.readout.is_some());
        let readout = studio
            .app_mut()
            .world()
            .resource::<OverlayStatus>()
            .readout
            .clone()
            .unwrap();
        assert!(readout.text.starts_with("slope "), "{}", readout.text);
        let legend_reads = |studio: &mut HeadlessStudio, line: &str| {
            studio
                .texts()
                .iter()
                .any(|t| t.text.lines().any(|l| l == line))
        };
        let line = format!(
            "Under the pointer: {} at ({:.0}, {:.0})",
            readout.text, readout.point.x, readout.point.y
        );
        assert!(
            legend_reads(&mut studio, &line),
            "the legend shows {line:?}"
        );

        // Other overlays read the same point their own way.
        draw(&mut studio, Overlay::Regions);
        wait_for(&mut studio, |status| {
            status
                .readout
                .as_ref()
                .is_some_and(|r| r.text.starts_with("region "))
        });
        draw(&mut studio, Overlay::Landform);
        wait_for(&mut studio, |status| {
            status
                .readout
                .as_ref()
                .is_some_and(|r| r.text.starts_with("landform "))
        });
        assert!(
            studio.find("riverbed").is_ok(),
            "the legend lists landforms"
        );

        // Off hides the legend.
        studio.command("overlay-off").unwrap();
        wait_for(&mut studio, |status| status.drawn.is_none());
        assert!(!studio.shows_text_starting("Cost: "));
    }
}
