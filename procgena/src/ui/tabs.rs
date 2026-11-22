//! A segmented row of buttons where exactly one is selected.

use bevy::feathers::controls::{ButtonVariant, FeathersButton};
use bevy::feathers::display::caption;
use bevy::feathers::rounded_corners::RoundedCorners;
use bevy::prelude::*;
use bevy::scene::Scene;
use bevy::ui_widgets::Activate;

/// A tab bar. Holds the index of the selected tab.
#[derive(Component, Default, Clone, Debug)]
pub struct TabBar {
    /// Index of the selected tab, counting from the left.
    pub selected: usize,
}

/// One tab button, with its index in the bar.
#[derive(Component, Default, Clone)]
struct TabButton(usize);

/// Triggered on a [`TabBar`] when the user picks a different tab.
#[derive(EntityEvent, Debug)]
pub struct TabSelected {
    /// The tab bar.
    pub entity: Entity,
    /// Index of the newly selected tab.
    pub index: usize,
}

/// Registers the observer that makes tab buttons work.
pub struct TabsPlugin;

impl Plugin for TabsPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(select_tab);
    }
}

/// A tab bar with one button per label, with tab `selected` picked.
/// Observe [`TabSelected`] on it to learn when the user picks another tab.
pub fn tab_bar(labels: &[&'static str], selected: usize) -> impl Scene {
    let last = labels.len().saturating_sub(1);
    let buttons: Vec<Box<dyn Scene>> = labels
        .iter()
        .enumerate()
        .map(|(index, label)| -> Box<dyn Scene> {
            let label = *label;
            let corners = match index {
                0 if last == 0 => RoundedCorners::All,
                0 => RoundedCorners::Left,
                i if i == last => RoundedCorners::Right,
                _ => RoundedCorners::None,
            };
            let variant = ButtonVariant::selected(index == selected);
            Box::new(bsn! {
                @FeathersButton {
                    @caption: bsn! { @caption(label) },
                    @variant: variant,
                    @corners: corners,
                }
                Node { flex_grow: 1.0 }
                TabButton({index})
            })
        })
        .collect();
    bsn! {
        Node {
            display: Display::Flex,
            flex_direction: FlexDirection::Row,
            flex_shrink: 0.0,
            column_gap: px(1),
        }
        TabBar { selected: {selected} }
        Children [ {buttons} ]
    }
}

/// Selects the pressed tab: highlights it, and reports it if it changed.
fn select_tab(
    activate: On<Activate>,
    buttons: Query<(&TabButton, &ChildOf)>,
    mut bars: Query<(&mut TabBar, &Children)>,
    mut variants: Query<(&TabButton, &mut ButtonVariant)>,
    mut commands: Commands,
) {
    let Ok((button, child_of)) = buttons.get(activate.event_target()) else {
        return;
    };
    let bar_entity = child_of.parent();
    let Ok((mut bar, children)) = bars.get_mut(bar_entity) else {
        return;
    };
    if bar.selected == button.0 {
        return;
    }
    bar.selected = button.0;

    // Highlight only the selected tab.
    for child in children.iter() {
        if let Ok((tab, mut variant)) = variants.get_mut(child) {
            variant.set_if_neq(ButtonVariant::selected(tab.0 == button.0));
        }
    }
    commands.trigger(TabSelected {
        entity: bar_entity,
        index: button.0,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tabs reported by [`TabSelected`], as a resource.
    #[derive(Resource, Default)]
    struct Picked(Vec<usize>);

    #[test]
    fn pressing_a_tab_selects_and_reports_it_once() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TabsPlugin))
            .init_resource::<Picked>()
            .add_observer(|picked: On<TabSelected>, mut log: ResMut<Picked>| {
                log.0.push(picked.index);
            });
        let world = app.world_mut();
        let bar = world.spawn(TabBar { selected: 0 }).id();
        let tabs: Vec<Entity> = (0..3)
            .map(|i| {
                world
                    .spawn((TabButton(i), ButtonVariant::selected(i == 0), ChildOf(bar)))
                    .id()
            })
            .collect();
        // Plugins' observers take effect on the first update.
        app.update();

        app.world_mut().trigger(Activate { entity: tabs[2] });
        app.world_mut().trigger(Activate { entity: tabs[2] });
        app.update();

        assert_eq!(app.world().resource::<Picked>().0, vec![2]);
        assert_eq!(app.world().get::<TabBar>(bar).unwrap().selected, 2);
        let variants: Vec<ButtonVariant> = tabs
            .iter()
            .map(|tab| app.world().get::<ButtonVariant>(*tab).unwrap().clone())
            .collect();
        assert_eq!(
            variants,
            [
                ButtonVariant::Normal,
                ButtonVariant::Normal,
                ButtonVariant::Primary
            ]
        );
    }
}
