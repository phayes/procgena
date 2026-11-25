//! The Ecotope editor's ground layers: one row per layer, top of the stack
//! first, with buttons to move a layer up or down the stack and remove it;
//! an Add Layer button; and the selected layer's settings, including which
//! Scatter it follows.

use bevy::feathers::constants::icons;
use bevy::feathers::controls::{
    ButtonVariant, FeathersButton, FeathersSelect, OptionIndex, list_rows_from_strings,
};
use bevy::feathers::display::{caption, icon, label_small};
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::{Activate, ValueChange};
use procgena_biome::Ecotope;
use procgena_core::ChildKey;
use procgena_terrain::ground::GroundLayer;

use super::{EcotopeEdited, EcotopeEditor};
use crate::ecotope::ground_params::{MAX_ORDER, layer_sections, new_influence};
use crate::params::{ParamPanel, ParamsChanged, spawn_param_panel};
use crate::studio::Dormant;
use crate::ui::labelled_row;

/// The picker entry for a layer that follows no Scatter.
const NO_SCATTER: &str = "None";

/// Marks a ground layer's row, with the layer's key.
#[derive(Component, Debug, Clone)]
pub(super) struct LayerRow(ChildKey);

/// Marks a row's move button: up the stack (towards the top) or down.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(super) struct MoveLayer {
    up: bool,
}

/// Marks the selected layer's settings panel.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(super) struct LayerPanel;

/// The Ecotope's ground layers, top of the stack first.
pub(super) fn layers_top_first(ecotope: &Ecotope) -> Vec<(&ChildKey, &GroundLayer)> {
    let mut layers: Vec<_> = ecotope.ground.iter().collect();
    layers.sort_by_key(|(key, layer)| (std::cmp::Reverse(layer.order), (*key).clone()));
    layers
}

/// Spawns the section's heading and Add Layer button as children of
/// `parent`, then the node for the rows and the node for the selected
/// layer's settings. Returns those two nodes.
pub(super) fn spawn_section(commands: &mut Commands, parent: Entity, column: &Node) -> (Entity, Entity) {
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(6),
                margin: UiRect::top(px(6)),
            }
            Children [
                Node { flex_grow: 1.0 }
                Children [ @label_small("Ground Layers (top first)") ]
                --
                @FeathersButton { @caption: bsn! { @caption("+ Add Layer") } }
                on(add_layer)
            ]
        })
        .insert(ChildOf(parent));
    let list = commands.spawn((column.clone(), ChildOf(parent))).id();
    let detail = commands.spawn((column.clone(), ChildOf(parent))).id();
    (list, detail)
}

/// Spawns one row per layer in `editor`'s ground list, which must be
/// empty, and if `with_detail`, the selected layer's settings in its ground
/// detail node, which must be empty too.
pub(super) fn spawn_layers(commands: &mut Commands, editor: &EcotopeEditor, with_detail: bool) {
    for (key, layer) in layers_top_first(&editor.ecotope) {
        let selected = editor.layer.as_ref() == Some(key);
        spawn_row(commands, editor.ground_list, key, layer, selected);
    }
    let Some(layer) = editor.layer.as_ref().and_then(|key| editor.ecotope.ground.get(key)) else {
        return;
    };
    if !with_detail {
        return;
    }

    // Which Scatter the layer follows, then its settings.
    let detail = editor.ground_detail;
    let mut scatters = vec![NO_SCATTER.to_owned()];
    scatters.extend(editor.ecotope.scatters.keys().map(ToString::to_string));
    let followed = layer
        .under
        .first()
        .and_then(|i| scatters.iter().position(|s| *s == i.scatter.as_str()))
        .unwrap_or(0);
    let picker: Box<dyn Scene> = Box::new(bsn! {
        @FeathersSelect { @options: list_rows_from_strings(scatters, Some(followed)) }
        Node { flex_grow: 1.0, min_width: px(0) }
        on(choose_followed_scatter)
    });
    commands
        .spawn_scene(bsn! { @labelled_row("Follows", picker) })
        .insert(ChildOf(detail));
    let panel = spawn_param_panel(commands, detail, layer.clone(), layer_sections(), &[0]);
    commands.entity(panel).insert(LayerPanel).observe(store_layer);
}

/// Spawns a row for layer `key` as the last child of `list`: a button
/// selecting it, its material and order, buttons moving it up and down the
/// stack, and a button removing it.
fn spawn_row(commands: &mut Commands, list: Entity, key: &ChildKey, layer: &GroundLayer, selected: bool) {
    let name = key.to_string();
    let about = format!("{} · {}", layer.material, layer.order);
    let variant = ButtonVariant::selected(selected);
    commands
        .spawn_scene(bsn! {
            Node {
                display: Display::Flex,
                flex_direction: FlexDirection::Row,
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: px(4),
            }
            Children [
                @FeathersButton { @caption: bsn! { @caption(name) }, @variant: variant }
                Node { flex_grow: 1.0, min_width: px(0) }
                on(select_layer)
                --
                Node { width: px(130), flex_shrink: 0.0, justify_content: JustifyContent::End }
                Children [ @label_small(about) ]
                --
                @FeathersButton {
                    @caption: bsn! { @caption("↑") },
                    @variant: ButtonVariant::Plain,
                }
                MoveLayer { up: true }
                on(move_layer)
                --
                @FeathersButton {
                    @caption: bsn! { @caption("↓") },
                    @variant: ButtonVariant::Plain,
                }
                MoveLayer { up: false }
                on(move_layer)
                --
                @FeathersButton {
                    @caption: bsn! { @icon(icons::X) },
                    @variant: ButtonVariant::Plain,
                }
                on(remove_layer)
            ]
        })
        .insert((LayerRow(key.clone()), ChildOf(list)));
}

/// The layer whose row holds `target`.
fn row_of(target: Entity, parents: &Query<&ChildOf>, rows: &Query<&LayerRow>) -> Option<ChildKey> {
    parents
        .iter_ancestors(target)
        .find_map(|ancestor| rows.get(ancestor).ok())
        .map(|row| row.0.clone())
}

/// Shows the settings of the layer whose row was clicked.
fn select_layer(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&LayerRow>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
) {
    let mut editor = editor.into_inner();
    let Some(key) = row_of(activate.event_target(), &parents, &rows) else {
        return;
    };
    if editor.layer.as_ref() != Some(&key) {
        editor.layer = Some(key);
        editor.stale.ground = true;
    }
}

/// Removes the layer whose row's button was pressed.
fn remove_layer(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&LayerRow>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Some(key) = row_of(activate.event_target(), &parents, &rows) else {
        return;
    };
    editor.ecotope.ground.remove(&key);
    if editor.layer.as_ref() == Some(&key) {
        editor.layer = layers_top_first(&editor.ecotope)
            .first()
            .map(|(key, _)| (*key).clone());
    }
    editor.stale.ground = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Moves a layer one place up or down the stack, by swapping its order
/// with its neighbour's.
fn move_layer(
    activate: On<Activate>,
    parents: Query<&ChildOf>,
    rows: Query<&LayerRow>,
    moves: Query<&MoveLayer>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let target = activate.event_target();
    let (Some(key), Ok(step)) = (row_of(target, &parents, &rows), moves.get(target)) else {
        return;
    };
    if swap_with_neighbour(&mut editor.ecotope, &key, step.up) {
        editor.stale.ground = true;
        commands.trigger(EcotopeEdited { reframe: false });
    }
}

/// Swaps layer `key`'s order with the next layer's up the stack (`up`) or
/// down it. Returns whether anything moved: not for the top layer going up
/// or the bottom one going down.
pub(super) fn swap_with_neighbour(ecotope: &mut Ecotope, key: &ChildKey, up: bool) -> bool {
    let layers: Vec<ChildKey> = layers_top_first(ecotope).into_iter().map(|(k, _)| k.clone()).collect();
    let Some(index) = layers.iter().position(|k| k == key) else {
        return false;
    };
    let neighbour = if up { index.checked_sub(1) } else { Some(index + 1) };
    let Some(neighbour) = neighbour.and_then(|i| layers.get(i)) else {
        return false;
    };
    let (a, b) = (ecotope.ground[key].order, ecotope.ground[neighbour].order);
    // Layers of equal order would clash, so keep them apart by one.
    let (a, b) = if a == b {
        if up { (a + 1, b) } else { (a, b + 1) }
    } else {
        (b, a)
    };
    ecotope.ground.get_mut(key).expect("the layer exists").order = a;
    ecotope.ground.get_mut(neighbour).expect("the neighbour exists").order = b;
    true
}

/// Adds a moss layer on top of the stack and shows its settings.
fn add_layer(_: On<Activate>, editor: Single<&mut EcotopeEditor, Without<Dormant>>, mut commands: Commands) {
    let mut editor = editor.into_inner();
    let key = add_new_layer(&mut editor.ecotope);
    editor.layer = Some(key);
    editor.stale.ground = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Adds a moss layer 10 above the top of `ecotope`'s stack, under the first
/// free key `layer`, `layer_2` and so on, and returns its key.
pub(super) fn add_new_layer(ecotope: &mut Ecotope) -> ChildKey {
    let top = ecotope.ground.values().map(|l| l.order).max();
    let order = top.map_or(100, |top| (top + 10).min(MAX_ORDER));
    let key = (1..)
        .map(|n| if n == 1 { "layer".to_owned() } else { format!("layer_{n}") })
        .map(|name| ChildKey::new(name).expect("layer keys are well formed"))
        .find(|key| !ecotope.ground.contains_key(key))
        .expect("some key is free");
    let material = ChildKey::new("moss").expect("a well-formed material key");
    ecotope.ground.insert(key.clone(), GroundLayer::new(material, order));
    key
}

/// Stores the selected layer's edited settings.
fn store_layer(
    changed: On<ParamsChanged>,
    panels: Query<&ParamPanel<GroundLayer>>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let (Ok(panel), Some(key)) = (panels.get(changed.entity), editor.layer.clone()) else {
        return;
    };
    let Some(layer) = editor.ecotope.ground.get_mut(&key) else {
        return;
    };
    // The rows show the material and order, and are in order.
    let relist = layer.material != panel.value().material || layer.order != panel.value().order;
    *layer = panel.value().clone();
    if relist {
        editor.stale.ground_list = true;
    }
    commands.trigger(EcotopeEdited { reframe: false });
}

/// Makes the selected layer follow the chosen Scatter, or none.
fn choose_followed_scatter(
    change: On<ValueChange<Entity>>,
    options: Query<&OptionIndex>,
    editor: Single<&mut EcotopeEditor, Without<Dormant>>,
    mut commands: Commands,
) {
    let mut editor = editor.into_inner();
    let Ok(index) = options.get(change.value) else {
        return;
    };
    let scatter = index
        .0
        .checked_sub(1)
        .and_then(|i| editor.ecotope.scatters.keys().nth(i).cloned());
    let Some(key) = editor.layer.clone() else {
        return;
    };
    let Some(layer) = editor.ecotope.ground.get_mut(&key) else {
        return;
    };
    let unchanged = layer.under.first().map(|i| &i.scatter) == scatter.as_ref();
    if unchanged {
        return;
    }
    match scatter {
        Some(scatter) => match layer.under.first_mut() {
            Some(influence) => influence.scatter = scatter,
            None => layer.under.push(new_influence(scatter)),
        },
        None => layer.under.clear(),
    }
    editor.stale.ground = true;
    commands.trigger(EcotopeEdited { reframe: false });
}

#[cfg(test)]
mod tests {
    use procgena_biome::EcotopePreset;

    use super::*;

    fn key(k: &str) -> ChildKey {
        ChildKey::new(k).unwrap()
    }

    #[test]
    fn layers_move_up_and_down_the_stack_by_swapping_orders() {
        let mut ecotope = EcotopePreset::pine_forest_floor().ecotope.clone();
        let names = |e: &Ecotope| -> Vec<String> {
            layers_top_first(e).iter().map(|(k, _)| k.to_string()).collect()
        };
        assert_eq!(names(&ecotope), ["dirt", "litter", "moss", "soil"]);
        assert!(swap_with_neighbour(&mut ecotope, &key("moss"), true));
        assert_eq!(names(&ecotope), ["dirt", "moss", "litter", "soil"]);
        assert!(!swap_with_neighbour(&mut ecotope, &key("dirt"), true));
        assert!(!swap_with_neighbour(&mut ecotope, &key("soil"), false));
        ecotope.validate().expect("the moved stack is still valid");
    }

    #[test]
    fn added_layers_go_on_top_under_free_keys() {
        let mut ecotope = EcotopePreset::pine_forest_floor().ecotope.clone();
        let first = add_new_layer(&mut ecotope);
        let second = add_new_layer(&mut ecotope);
        assert_eq!((first.as_str(), second.as_str()), ("layer", "layer_2"));
        assert_eq!(layers_top_first(&ecotope)[0].0, &second);
        ecotope.validate().expect("added layers keep the stack valid");
    }
}
