//! A feathers panel generated at runtime from a list of [`Section`]s.
//!
//! Each setting becomes one feathers control. Edits go to the
//! [`ParamPanel`] that owns the control, which then triggers [`ParamsChanged`].
//! Register [`ParamPanelPlugin`] once for each edited type, and
//! [`PanelWidgetsPlugin`] once per app.

use std::collections::HashSet;
use std::marker::PhantomData;
use std::sync::Arc;

use bevy::feathers::controls::{
    ColorInputValue, FeathersColorInput, FeathersDisclosureToggle, FeathersSelect, FeathersSlider,
    FeathersToggleSwitch, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{label, label_small};
use bevy::picking::events::PointerClick;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui::Checked;
use bevy::ui_widgets::{SliderPrecision, SliderStep, SliderValue, ValueChange};

use crate::params::spec::{Control, Param, Section, decimals};
use crate::params::{srgb_from_hex, srgb_to_hex};
use crate::ui::{SelectOptions, labelled_row};

/// Height of number sliders in logical pixels; flatter than the feathers default of 24.
const SLIDER_HEIGHT: f32 = 18.0;

/// The value a panel edits, and the settings it shows.
#[derive(Component)]
pub struct ParamPanel<T: Send + Sync + 'static> {
    value: T,
    sections: Arc<Vec<Section<T>>>,
    hidden: HashSet<&'static str>,
}

impl<T: Send + Sync + 'static> ParamPanel<T> {
    /// The value being edited.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Replaces the value being edited; every control shows it from the next frame.
    /// Does not trigger [`ParamsChanged`].
    pub fn set_value(&mut self, value: T) {
        self.value = value;
    }

    /// Hides the settings whose [`Param::key`] is listed, without changing their
    /// values. Replaces the previous list; settings no longer listed follow their
    /// usual visibility rules.
    pub fn set_hidden_keys(&mut self, keys: impl IntoIterator<Item = &'static str>) {
        self.hidden = keys.into_iter().collect();
    }

    /// The setting at flat index `index`, counting every section's settings in order.
    fn param(&self, index: usize) -> Option<&Param<T>> {
        self.sections.iter().flat_map(Section::params).nth(index)
    }

    /// Whether the setting at flat index `index` is shown for the current value.
    fn shows(&self, index: usize) -> bool {
        self.param(index).is_some_and(|param| {
            param.is_shown(&self.value) && !self.hidden.contains(param.key.as_str())
        })
    }
}

/// Triggered on a panel entity after the user changes one of its settings.
#[derive(EntityEvent)]
pub struct ParamsChanged {
    /// The panel whose value changed.
    pub entity: Entity,
}

/// Marks a control entity with the flat index of the setting it edits.
#[derive(Component, Default, Clone)]
struct ParamControl {
    index: usize,
}

/// Marks the labelled row of a setting, which is hidden when the setting is.
#[derive(Component, Default, Clone)]
struct ParamRow {
    index: usize,
}

/// Marks a group's sub-heading, hidden when none of the group's `len`
/// settings, starting at flat index `first`, is shown.
#[derive(Component, Default, Clone)]
struct GroupTitle {
    first: usize,
    len: usize,
}

/// Marks a section body, shown while its disclosure toggle is checked.
#[derive(Component, Default, Clone)]
struct SectionBody;

/// Registers the observers and systems that make `ParamPanel<T>` work.
pub struct ParamPanelPlugin<T>(PhantomData<T>);

impl<T> Default for ParamPanelPlugin<T> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

impl<T: Clone + PartialEq + Send + Sync + 'static> Plugin for ParamPanelPlugin<T> {
    fn build(&self, app: &mut App) {
        app.add_observer(on_number::<T>)
            .add_observer(on_toggle::<T>)
            .add_observer(on_choice::<T>)
            .add_observer(on_color::<T>)
            .add_systems(PostUpdate, sync_panel::<T>);
    }
}

/// Registers the panel-independent observers (section disclosure).
pub struct PanelWidgetsPlugin;

impl Plugin for PanelWidgetsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_section_toggle)
            .add_observer(on_header_click);
    }
}

/// Spawns a panel editing `value` as a child of `parent` and returns it.
/// Sections whose indices are in `open` start expanded.
pub fn spawn_param_panel<T: Clone + PartialEq + Send + Sync + 'static>(
    commands: &mut Commands,
    parent: Entity,
    value: T,
    sections: Vec<Section<T>>,
    open: &[usize],
) -> Entity {
    // Build every section's scene, numbering settings across sections.
    let mut index = 0;
    let mut section_scenes: Vec<Box<dyn Scene>> = Vec::new();
    for (section_index, section) in sections.iter().enumerate() {
        let mut rows: Vec<Box<dyn Scene>> = Vec::new();
        for group in &section.groups {
            if let Some(title) = group.title {
                rows.push(group_title_scene(title, index, group.params.len()));
            }
            for param in &group.params {
                rows.push(row_scene(param, &value, index));
                index += 1;
            }
        }
        section_scenes.push(section_scene(
            section.title,
            open.contains(&section_index),
            rows,
        ));
    }

    // Spawn the panel and attach the edited value.
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(6),
            }
            Children [ {section_scenes} ]
        })
        .insert((
            ParamPanel {
                value,
                sections: Arc::new(sections),
                hidden: HashSet::new(),
            },
            ChildOf(parent),
        ))
        .id()
}

/// A collapsible section: a disclosure header over its rows.
fn section_scene(title: &'static str, open: bool, rows: Vec<Box<dyn Scene>>) -> Box<dyn Scene> {
    let toggle: Box<dyn Scene> = if open {
        Box::new(bsn! { @FeathersDisclosureToggle Checked })
    } else {
        Box::new(bsn! { @FeathersDisclosureToggle })
    };
    let display = if open { Display::Flex } else { Display::None };
    Box::new(bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Column,
            flex_shrink: 0.0,
            row_gap: px(4),
        }
        Children [
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(6),
            }
            SectionHeader
            Children [ {vec![toggle]} -- @label(title) ]
            --
            Node {
                display: {display},
                flex_direction: FlexDirection::Column,
                flex_shrink: 0.0,
                row_gap: px(4),
                padding: UiRect::left(px(18)),
            }
            SectionBody
            Children [ {rows} ]
        ]
    })
}

/// Marks a section's header row: its disclosure toggle and title.
#[derive(Component, Default, Clone)]
struct SectionHeader;

/// A group's sub-heading, covering the `len` settings from flat index `first`.
fn group_title_scene(title: &'static str, first: usize, len: usize) -> Box<dyn Scene> {
    Box::new(bsn! {
        Node {
            flex_shrink: 0.0,
            padding: UiRect::top(px(4)),
        }
        GroupTitle { first: {first}, len: {len} }
        Children [ @label_small(title) ]
    })
}

/// One labelled control for `param`.
fn row_scene<T>(param: &Param<T>, value: &T, index: usize) -> Box<dyn Scene> {
    let text = param.label.clone();
    let control = control_scene(param, value, index);
    Box::new(bsn! {
        @labelled_row(text, control)
        ParamRow { index: {index} }
    })
}

/// The feathers control for `param`, showing its value in `value`.
fn control_scene<T>(param: &Param<T>, value: &T, index: usize) -> Box<dyn Scene> {
    match &param.control {
        Control::Number {
            min,
            max,
            step,
            get,
            ..
        } => {
            let (min, max, step) = (*min, *max, *step);
            let current = get(value);
            let precision = decimals(step) as i32;
            Box::new(bsn! {
                @FeathersSlider { @min: min, @max: max }
                Node { height: px(SLIDER_HEIGHT) }
                SliderValue({current})
                SliderStep({step})
                SliderPrecision({precision})
                ParamControl { index: {index} }
            })
        }
        Control::Toggle { get, .. } => {
            if get(value) {
                Box::new(bsn! { @FeathersToggleSwitch Checked ParamControl { index: {index} } })
            } else {
                Box::new(bsn! { @FeathersToggleSwitch ParamControl { index: {index} } })
            }
        }
        Control::Choice { options, get, .. } => {
            let options = *options;
            let selected = get(value);
            Box::new(bsn! {
                @FeathersSelect { @options: list_rows_from_strings(options.iter(), Some(selected)) }
                Node { flex_grow: 1.0 }
                ParamControl { index: {index} }
            })
        }
        Control::Color { get, .. } => {
            let color = srgb_from_hex(get(value));
            Box::new(bsn! {
                @FeathersColorInput
                ColorInputValue({color})
                ParamControl { index: {index} }
            })
        }
    }
}

/// Finds the `ParamPanel<T>` that `entity` belongs to.
fn owning_panel<T: Send + Sync + 'static>(
    entity: Entity,
    parents: &Query<&ChildOf>,
    panels: &Query<&mut ParamPanel<T>>,
) -> Option<Entity> {
    parents
        .iter_ancestors(entity)
        .find(|ancestor| panels.contains(*ancestor))
}

/// Applies `edit` to the panel owning `source` and reports a change.
fn edit_panel<T: Clone + PartialEq + Send + Sync + 'static>(
    source: Entity,
    parents: &Query<&ChildOf>,
    panels: &mut Query<&mut ParamPanel<T>>,
    commands: &mut Commands,
    edit: impl FnOnce(&Control<T>, &mut T),
    index: usize,
) {
    let Some(panel_entity) = owning_panel(source, parents, panels) else {
        return;
    };
    let Ok(mut panel) = panels.get_mut(panel_entity) else {
        return;
    };
    // Hold the spec separately so the value can be edited while reading it.
    let sections = panel.sections.clone();
    let Some(param) = sections.iter().flat_map(Section::params).nth(index) else {
        return;
    };
    let mut value = panel.value.clone();
    edit(&param.control, &mut value);
    if value != panel.value {
        panel.value = value;
        commands.trigger(ParamsChanged {
            entity: panel_entity,
        });
    }
}

/// Number sliders.
fn on_number<T: Clone + PartialEq + Send + Sync + 'static>(
    change: On<ValueChange<f32>>,
    controls: Query<&ParamControl>,
    parents: Query<&ChildOf>,
    mut panels: Query<&mut ParamPanel<T>>,
    mut commands: Commands,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let number = change.value;
    edit_panel(
        change.source,
        &parents,
        &mut panels,
        &mut commands,
        |control, value| {
            if let Control::Number { set, .. } = control {
                set(value, number);
            }
        },
        control.index,
    );
}

/// Colour pickers. The picker's alpha is ignored; colours are stored opaque.
fn on_color<T: Clone + PartialEq + Send + Sync + 'static>(
    change: On<ValueChange<Color>>,
    controls: Query<&ParamControl>,
    parents: Query<&ChildOf>,
    mut panels: Query<&mut ParamPanel<T>>,
    mut commands: Commands,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let hex = srgb_to_hex(change.value);
    edit_panel(
        change.source,
        &parents,
        &mut panels,
        &mut commands,
        |control, value| {
            if let Control::Color { set, .. } = control {
                set(value, hex);
            }
        },
        control.index,
    );
}

/// Toggle switches.
fn on_toggle<T: Clone + PartialEq + Send + Sync + 'static>(
    change: On<ValueChange<bool>>,
    controls: Query<&ParamControl>,
    parents: Query<&ChildOf>,
    mut panels: Query<&mut ParamPanel<T>>,
    mut commands: Commands,
) {
    let Ok(control) = controls.get(change.source) else {
        return;
    };
    let checked = change.value;
    edit_panel(
        change.source,
        &parents,
        &mut panels,
        &mut commands,
        |control, value| {
            if let Control::Toggle { set, .. } = control {
                set(value, checked);
            }
        },
        control.index,
    );
}

/// Drop-down choices.
fn on_choice<T: Clone + PartialEq + Send + Sync + 'static>(
    change: On<ValueChange<Entity>>,
    controls: Query<&ParamControl>,
    rows: Query<&OptionIndex>,
    parents: Query<&ChildOf>,
    mut panels: Query<&mut ParamPanel<T>>,
    mut commands: Commands,
) {
    let (Ok(control), Ok(choice)) = (controls.get(change.source), rows.get(change.value)) else {
        return;
    };
    let choice = choice.0;
    edit_panel(
        change.source,
        &parents,
        &mut panels,
        &mut commands,
        |control, value| {
            if let Control::Choice { set, .. } = control {
                set(value, choice);
            }
        },
        control.index,
    );
}

/// Expands or collapses a section when its header, outside the toggle, is clicked.
fn on_header_click(
    click: On<PointerClick>,
    headers: Query<&Children, With<SectionHeader>>,
    toggles: Query<Has<Checked>, With<FeathersDisclosureToggle>>,
    mut commands: Commands,
) {
    let Ok(children) = headers.get(click.entity) else {
        return;
    };
    for &child in children {
        if let Ok(checked) = toggles.get(child) {
            commands.trigger(ValueChange {
                source: child,
                value: !checked,
                is_final: true,
            });
        }
    }
}

/// Expands or collapses a section when its disclosure toggle changes.
fn on_section_toggle(
    change: On<ValueChange<bool>>,
    toggles: Query<(), With<FeathersDisclosureToggle>>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    mut bodies: Query<&mut Node, With<SectionBody>>,
    mut commands: Commands,
) {
    if !toggles.contains(change.source) {
        return;
    }
    if change.value {
        commands.entity(change.source).insert(Checked);
    } else {
        commands.entity(change.source).remove::<Checked>();
    }
    // The toggle sits in the header, whose sibling is the body.
    let Some(section) = parents.iter_ancestors(change.source).nth(1) else {
        return;
    };
    for child in children.get(section).into_iter().flatten() {
        if let Ok(mut node) = bodies.get_mut(*child) {
            node.display = if change.value {
                Display::Flex
            } else {
                Display::None
            };
        }
    }
}

/// Shows `node` when `shown` holds and hides it otherwise, touching it only on change.
fn set_shown(node: &mut Mut<Node>, shown: bool) {
    let display = if shown { Display::Flex } else { Display::None };
    if node.display != display {
        node.display = display;
    }
}

/// Brings controls, rows and sub-headings in line with each changed panel's value.
#[allow(clippy::type_complexity)]
fn sync_panel<T: Send + Sync + 'static>(
    panels: Query<(Entity, &ParamPanel<T>), Changed<ParamPanel<T>>>,
    descendants: Query<&Children>,
    mut nodes: Query<(&mut Node, Option<&ParamRow>, Option<&GroupTitle>)>,
    controls: Query<(
        &ParamControl,
        Option<&SliderValue>,
        Option<&ColorInputValue>,
        Has<Checked>,
        Has<FeathersToggleSwitch>,
        Has<FeathersSelect>,
    )>,
    select_options: SelectOptions,
    mut commands: Commands,
) {
    for (panel_entity, panel) in &panels {
        for entity in descendants.iter_descendants(panel_entity) {
            // Hide settings with no effect or hidden by key, and empty sub-headings.
            if let Ok((mut node, row, title)) = nodes.get_mut(entity) {
                if let Some(row) = row {
                    set_shown(&mut node, panel.shows(row.index));
                }
                if let Some(title) = title {
                    let any = (title.first..title.first + title.len).any(|i| panel.shows(i));
                    set_shown(&mut node, any);
                }
            }

            // Show the current value on each control.
            let Ok((control, slider, color_input, checked, is_switch, is_select)) =
                controls.get(entity)
            else {
                continue;
            };
            let Some(param) = panel.param(control.index) else {
                continue;
            };
            match &param.control {
                Control::Number { get, .. } => {
                    let number = get(&panel.value);
                    if slider.is_some_and(|s| s.0 != number) {
                        commands.entity(entity).insert(SliderValue(number));
                    }
                }
                Control::Toggle { get, .. } if is_switch => {
                    let on = get(&panel.value);
                    if on && !checked {
                        commands.entity(entity).insert(Checked);
                    } else if !on && checked {
                        commands.entity(entity).remove::<Checked>();
                    }
                }
                Control::Color { get, .. } => {
                    // Compare as hex so the picker's own unrounded value isn't overwritten.
                    let hex = get(&panel.value);
                    if color_input.is_some_and(|shown| srgb_to_hex(shown.0) != hex) {
                        commands
                            .entity(entity)
                            .insert(ColorInputValue(srgb_from_hex(hex)));
                    }
                }
                Control::Choice { get, .. } if is_select => {
                    select_options.select(&mut commands, entity, get(&panel.value));
                }
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::{Group, number, toggle};

    #[derive(Clone, Copy, PartialEq, Debug, Default)]
    struct Demo {
        on: bool,
        size: f32,
    }

    /// One section: a toggle, then a group titled "Size" whose number shows only while on.
    fn sections() -> Vec<Section<Demo>> {
        vec![Section {
            title: "Demo",
            groups: vec![
                Group::plain(vec![toggle!(Demo, "on", "On", |d| d.on)]),
                Group::titled(
                    "Size",
                    vec![
                        number!(Demo, "size", "Size", (0.0, 10.0, 0.5), |d| d.size)
                            .shown_if(|d| d.on),
                    ],
                ),
            ],
        }]
    }

    /// The edits reported by [`ParamsChanged`], as a resource.
    #[derive(Resource, Default)]
    struct Changes(usize);

    /// An app with a demo panel holding a slider control, a row and a sub-heading for the
    /// size setting. Returns the app, the panel, the slider, the size row and the heading.
    fn app() -> (App, Entity, Entity, Entity, Entity) {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, ParamPanelPlugin::<Demo>::default()))
            .init_resource::<Changes>()
            .add_observer(|_: On<ParamsChanged>, mut changes: ResMut<Changes>| {
                changes.0 += 1;
            });
        let world = app.world_mut();
        let panel = world
            .spawn(ParamPanel {
                value: Demo::default(),
                sections: Arc::new(sections()),
                hidden: HashSet::new(),
            })
            .id();
        let heading = world
            .spawn((
                Node::default(),
                GroupTitle { first: 1, len: 1 },
                ChildOf(panel),
            ))
            .id();
        let row = world
            .spawn((Node::default(), ParamRow { index: 1 }, ChildOf(panel)))
            .id();
        let slider = world
            .spawn((ParamControl { index: 1 }, SliderValue(0.0), ChildOf(row)))
            .id();
        // Plugins' observers take effect on the first update.
        app.update();
        (app, panel, slider, row, heading)
    }

    fn display(app: &App, entity: Entity) -> Display {
        app.world().get::<Node>(entity).unwrap().display
    }

    #[test]
    fn an_edit_applies_the_setter_and_reports_it() {
        let (mut app, panel, slider, _, _) = app();
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 2.5_f32,
            is_final: true,
        });
        let value = *app.world().get::<ParamPanel<Demo>>(panel).unwrap().value();
        assert_eq!(value.size, 2.5);
        // The report is queued; it arrives on the next update.
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 1);

        // Repeating the same value is not a change.
        app.world_mut().trigger(ValueChange {
            source: slider,
            value: 2.5_f32,
            is_final: true,
        });
        app.update();
        assert_eq!(app.world().resource::<Changes>().0, 1);
    }

    #[test]
    fn rows_and_headings_follow_the_value_and_hidden_keys() {
        let (mut app, panel, _, row, heading) = app();
        app.update();
        assert_eq!(display(&app, row), Display::None);
        assert_eq!(display(&app, heading), Display::None);

        let mut panel_ref = app.world_mut().get_mut::<ParamPanel<Demo>>(panel).unwrap();
        panel_ref.set_value(Demo {
            on: true,
            size: 0.0,
        });
        app.update();
        assert_eq!(display(&app, row), Display::Flex);
        assert_eq!(display(&app, heading), Display::Flex);

        let mut panel_ref = app.world_mut().get_mut::<ParamPanel<Demo>>(panel).unwrap();
        panel_ref.set_hidden_keys(["size"]);
        app.update();
        assert_eq!(display(&app, row), Display::None);
        assert_eq!(display(&app, heading), Display::None);
    }

    #[test]
    fn set_value_updates_controls_without_reporting_an_edit() {
        let (mut app, panel, slider, _, _) = app();
        let mut panel_ref = app.world_mut().get_mut::<ParamPanel<Demo>>(panel).unwrap();
        panel_ref.set_value(Demo {
            on: true,
            size: 7.0,
        });
        app.update();
        assert_eq!(app.world().get::<SliderValue>(slider).unwrap().0, 7.0);
        assert_eq!(app.world().resource::<Changes>().0, 0);
    }

    #[test]
    fn controls_outside_a_panel_are_ignored() {
        let (mut app, _, _, _, _) = app();
        let stray = app.world_mut().spawn(ParamControl { index: 1 }).id();
        app.world_mut().trigger(ValueChange {
            source: stray,
            value: 1.0_f32,
            is_final: true,
        });
        assert_eq!(app.world().resource::<Changes>().0, 0);
    }
}
