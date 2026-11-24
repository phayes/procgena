//! The Overlay control on the view frame's bar, and the overlay's legend
//! over the 3D view: what each colour means, what sampling costs, and the
//! value under the pointer.
//!
//! The control is shown only while the shown subject offers overlays (see
//! [`OverlayStatus`]). Scripts switch overlays with the `overlay-off` and
//! `overlay-<id>` commands, such as `overlay-slope`.

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

/// One `overlay-<id>` command per overlay.
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

/// The command names, in [`Overlay::ALL`] order; a test checks they match
/// [`Overlay::id`].
const OVERLAY_NAMES: [&str; Overlay::ALL.len()] = [
    "overlay-regions",
    "overlay-ground",
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
    match overlay.legend(&status.catalogue) {
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
            let ramp = commands
                .spawn((
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        column_gap: px(6),
                    },
                    ChildOf(colours),
                ))
                .id();
            commands.spawn_scene(label_small(low)).insert(ChildOf(ramp));
            let bar = commands
                .spawn((
                    Node {
                        display: Display::Flex,
                        flex_direction: FlexDirection::Row,
                    },
                    ChildOf(ramp),
                ))
                .id();
            for colour in stops {
                commands.spawn((
                    Node {
                        width: px(18),
                        height: px(10),
                    },
                    BackgroundColor(Color::srgb_u8(colour[0], colour[1], colour[2])),
                    ChildOf(bar),
                ));
            }
            commands.spawn_scene(label_small(high)).insert(ChildOf(ramp));
            for (colour, text) in extra {
                swatch_row(&mut commands, colours, colour, text);
            }
        }
    }
}

/// One swatch of `colour` with `text` beside it, under `parent`.
fn swatch_row(commands: &mut Commands, parent: Entity, colour: [u8; 3], text: String) {
    let row = commands
        .spawn((
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(6),
            },
            ChildOf(parent),
        ))
        .id();
    commands.spawn((
        Node {
            width: px(10),
            height: px(10),
        },
        BackgroundColor(Color::srgb_u8(colour[0], colour[1], colour[2])),
        ChildOf(row),
    ));
    commands.spawn_scene(label_small(text)).insert(ChildOf(row));
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
        _ => lines.push(format!("Cost: {texels}² texels per chunk, {}", family.cost())),
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
    use super::*;

    #[test]
    fn every_overlay_has_its_own_command() {
        for (overlay, name) in Overlay::ALL.iter().zip(OVERLAY_NAMES) {
            assert_eq!(name, format!("overlay-{}", overlay.id()));
        }
    }
}
