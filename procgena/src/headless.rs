//! Running the studio without a window, for checking the UI from scripts and tests.
//!
//! [`HeadlessStudio`] runs the whole studio, settings panel included, on the
//! calling thread. The window contents are drawn into an image, and mouse
//! input goes through Bevy's picking as if the mouse were on that image. A GPU
//! is still required.
//!
//! It runs frames the way the window does: one for each input, then more only
//! while something asks for a redraw. Screenshots show the last frame drawn, so
//! they show what the window would show once it goes idle.
//!
//! From the command line, `procgena --headless <script>` follows a script with one
//! command per line (`-` reads it from standard input). Blank lines and lines
//! starting with `#` are skipped. Coordinates are pixels from the top-left of
//! the window. Every command returns once the studio is idle again.
//!
//! | Command | Effect |
//! | --- | --- |
//! | `size W H` | Window size, before any other command. Default 1440 x 900. |
//! | `click TEXT` | Clicks the visible text exactly matching `TEXT`. |
//! | `click-at X Y` | Clicks at a point. |
//! | `drag X1 Y1 X2 Y2` | Drags with the left button between two points. |
//! | `scroll X Y LINES` | Turns the wheel at a point; positive scrolls up. |
//! | `key KEY` | Presses a key: `Escape`, `Enter`, `Tab`, `Backspace` or a letter, optionally after `ctrl+` or `cmd+`. |
//! | `type TEXT` | Types `TEXT` into the focused text field. Click the field first. |
//! | `hover X Y` | Moves the mouse to a point without clicking, for example to show a tooltip. |
//! | `select PATH` | Selects a document as `KIND/KEY[/NODE…][#PART]`, such as `ecotope/pine_forest_floor`, opening it if needed. |
//! | `command NAME` | Runs a studio command, such as `reset-camera` or `close-document`. |
//! | `wait SECONDS` | Lets time pass, drawing all the while, for example for a message to hide itself. |
//! | `choose PATH` | Answers the next file dialog with `PATH`. Give it before the click that opens the dialog. |
//! | `cancel-dialog` | Cancels the next file dialog. Dialogs with no answer given are cancelled too. |
//! | `pace MS` | Sleeps this long after every later frame, as a window waiting for the display does. |
//! | `shot PATH` | Saves the last frame drawn as a PNG. |
//! | `dump` | Prints every visible text with its centre and size. |

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::scene::headless::{headless_plugins, unpad_rows};
use bevy::app::PluginsState;
use bevy::asset::RenderAssetUsages;
use bevy::camera::{NormalizedRenderTarget, RenderTarget};
use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput, NativeKey};
use bevy::input::mouse::MouseScrollUnit;
use bevy::input::touch::TouchPhase;
use bevy::input_focus::{AcquireFocus, FocusedInput, InputFocus};
use bevy::picking::events::PointerPress;
use bevy::picking::pointer::{Location, PointerAction, PointerButton, PointerId, PointerInput};
use bevy::prelude::*;
use bevy::render::gpu_readback::{Readback, ReadbackComplete};
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};
use bevy::tasks::tick_global_task_pools_on_main_thread;
use bevy::text::EditableText;
use bevy::time::TimeUpdateStrategy;
use bevy::ui::{CalculatedClip, ComputedNode, UiGlobalTransform};
use bevy::window::RequestRedraw;

use crate::export::FileDialogs;

/// Window size when none is given.
pub const DEFAULT_SIZE: UVec2 = UVec2::new(1440, 900);

/// Time that passes per frame.
const FRAME_TIME: Duration = Duration::from_micros(16_667);

/// Steps a drag is split into.
const DRAG_STEPS: u32 = 8;

/// Most extra frames run to collect the readback of the last frame drawn.
const MAX_READBACK_FRAMES: u32 = 8;

/// Frames read back and kept, newest first; older ones can no longer be captured.
const KEPT_FRAMES: usize = 16;

/// The longest any wait in this module may take before it panics.
const LIMIT: Duration = Duration::from_secs(120);

/// How often to check on file operations working in the background.
const FILE_WORK_POLL: Duration = Duration::from_millis(5);

/// The image the studio's UI camera draws into.
#[derive(Resource, Clone)]
struct UiImage(Handle<Image>);

/// A frame's number, counting from 1 in drawing order, and its RGBA8 pixels.
type NumberedFrame = (u64, Vec<u8>);

/// Window frames read back so far.
#[derive(Resource, Clone, Default)]
struct FrameLog {
    /// The newest frames, oldest first.
    frames: Arc<Mutex<VecDeque<NumberedFrame>>>,
    /// Readbacks delivered so far, kept or not.
    delivered: Arc<Mutex<u64>>,
}

/// A visible piece of text, in window pixels.
#[derive(Debug, Clone, PartialEq)]
pub struct ShownText {
    /// The text.
    pub text: String,
    /// The centre of its box.
    pub center: Vec2,
    /// The size of its box.
    pub size: Vec2,
}

/// The studio running without a window on the calling thread.
///
/// Nothing happens between calls: each method runs as many frames as it
/// needs to. Input methods return once the studio is idle again.
pub struct HeadlessStudio {
    app: App,
    image: Handle<Image>,
    frames: FrameLog,
    size: UVec2,
    cursor: Vec2,
    /// Frames run so far, including those run only to collect readbacks.
    drawn: u64,
    /// The number of the last frame the window would have drawn.
    shown: u64,
    /// Wall-clock time slept after each frame.
    pace: Duration,
    /// Answers for the coming file dialogs, in order.
    dialog_answers: Arc<Mutex<VecDeque<Option<PathBuf>>>>,
}

impl HeadlessStudio {
    /// Starts the studio with a window of `size` pixels and runs it until idle.
    ///
    /// # Panics
    ///
    /// Panics if Bevy cannot start its renderer, for example without a GPU,
    /// or if the studio does not become idle within two minutes.
    pub fn new(size: UVec2) -> Self {
        let size = size.max(UVec2::ONE);
        let frames = FrameLog::default();
        let mut app = App::new();
        crate::scene::add_asset_sources(&mut app);
        app.add_plugins(headless_plugins());
        crate::add_studio(&mut app);
        let dialog_answers = Arc::new(Mutex::new(VecDeque::new()));
        app.insert_resource(FileDialogs::Scripted(dialog_answers.clone()))
            .insert_resource(TimeUpdateStrategy::ManualDuration(FRAME_TIME))
            .insert_resource(frames.clone())
            .add_systems(Startup, spawn_readback)
            .add_systems(PostStartup, retarget_ui_camera)
            .add_observer(click_to_focus);

        // Finish the plugins; the renderer starts asynchronously.
        while app.plugins_state() == PluginsState::Adding {
            tick_global_task_pools_on_main_thread();
        }
        app.finish();
        app.cleanup();

        // Draw the window contents into an image of the requested size.
        let mut image =
            Image::new_target_texture(size.x, size.y, TextureFormat::Rgba8UnormSrgb, None);
        image.texture_descriptor.usage |= TextureUsages::COPY_SRC;
        let image = app.world_mut().resource_mut::<Assets<Image>>().add(image);
        app.insert_resource(UiImage(image.clone()));

        let mut studio = Self {
            app,
            image,
            frames,
            size,
            cursor: Vec2::ZERO,
            drawn: 0,
            shown: 0,
            pace: Duration::ZERO,
            dialog_answers,
        };
        studio.settle();
        studio
    }

    /// The app, for anything this type does not cover.
    #[cfg(test)]
    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// Every visible text, in drawing order, including the contents of text
    /// fields. Text scrolled or clipped out of view is left out.
    pub fn texts(&mut self) -> Vec<ShownText> {
        let world = self.app.world_mut();
        let mut query = world.query::<(
            AnyOf<(&Text, &EditableText)>,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
            Option<&CalculatedClip>,
        )>();
        query
            .iter(world)
            .filter_map(|((text, field), node, transform, visible, clip)| {
                let center = shown_center(node, transform, visible, clip)?;
                // A text field may also carry an empty `Text`; its contents are what show.
                let text = match (field, text) {
                    (Some(field), _) => field.value().to_string(),
                    (None, Some(text)) => text.0.clone(),
                    (None, None) => return None,
                };
                (!text.is_empty()).then(|| ShownText {
                    text,
                    center,
                    size: node.size() * node.inverse_scale_factor(),
                })
            })
            .collect()
    }

    /// The centre of the visible text exactly matching `text`.
    ///
    /// # Errors
    ///
    /// Fails if no visible text, or more than one, matches.
    pub fn find(&mut self, text: &str) -> Result<Vec2, String> {
        let matches: Vec<ShownText> = self
            .texts()
            .into_iter()
            .filter(|shown| shown.text == text)
            .collect();
        match matches.as_slice() {
            [only] => Ok(only.center),
            [] => Err(format!("no visible text {text:?}")),
            _ => Err(format!("{} visible texts match {text:?}", matches.len())),
        }
    }

    /// The centre of `entity`'s box in window pixels, or `None` if it is not a
    /// visible UI node.
    #[cfg(test)]
    pub fn center_of(&mut self, entity: Entity) -> Option<Vec2> {
        let world = self.app.world_mut();
        let mut query = world.query::<(
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
            Option<&CalculatedClip>,
        )>();
        let (node, transform, visible, clip) = query.get(world, entity).ok()?;
        shown_center(node, transform, visible, clip)
    }

    /// The value of the only settings panel editing a `T` that is on
    /// screen: the selected document's, since other documents' panels are
    /// hidden.
    ///
    /// # Panics
    ///
    /// Panics unless exactly one such panel is on screen.
    #[cfg(test)]
    pub fn panel_value<T: Clone + Send + Sync + 'static>(&mut self) -> T {
        let world = self.app.world_mut();
        let mut panels = world.query::<(&crate::params::ParamPanel<T>, &InheritedVisibility)>();
        let shown: Vec<T> = panels
            .iter(world)
            .filter(|(_, visible)| visible.get())
            .map(|(panel, _)| panel.value().clone())
            .collect();
        let [value] = <[T; 1]>::try_from(shown)
            .unwrap_or_else(|shown| panic!("{} panels on screen edit this type", shown.len()));
        value
    }

    /// Selects `path`, opening its document if needed; see
    /// [`select_path`](crate::studio::commands::select_path).
    ///
    /// # Errors
    ///
    /// Fails as `select_path` does.
    pub fn select(&mut self, path: &str) -> Result<(), String> {
        crate::studio::commands::select_path(self.app.world_mut(), path)?;
        self.settle();
        Ok(())
    }

    /// Runs the studio command named `name`; see
    /// [`run_command`](crate::studio::commands::run_command).
    ///
    /// # Errors
    ///
    /// Fails if there is no such command.
    pub fn command(&mut self, name: &str) -> Result<(), String> {
        crate::studio::commands::run_command(self.app.world_mut(), name)?;
        self.settle();
        Ok(())
    }

    /// Whether a visible text starts with `start`, as toasts with details do.
    #[cfg(test)]
    pub fn shows_text_starting(&mut self, start: &str) -> bool {
        self.texts()
            .iter()
            .any(|shown| shown.text.starts_with(start))
    }

    /// Starts recording every message of type `M`; the returned list grows as
    /// the studio runs.
    #[cfg(test)]
    pub fn record<M: Message + Clone>(&mut self) -> Arc<Mutex<Vec<M>>> {
        let log = Arc::new(Mutex::new(Vec::new()));
        let sink = log.clone();
        self.app
            .add_systems(Last, move |mut messages: MessageReader<M>| {
                lock(&sink).extend(messages.read().cloned());
            });
        log
    }

    /// Clicks the visible text exactly matching `text`.
    ///
    /// # Errors
    ///
    /// Fails as [`find`](Self::find) does.
    pub fn click(&mut self, text: &str) -> Result<(), String> {
        let at = self.find(text)?;
        self.click_at(at);
        Ok(())
    }

    /// Clicks with the left button at `at`.
    pub fn click_at(&mut self, at: Vec2) {
        self.move_to(at);
        self.pointer(PointerAction::Press(PointerButton::Primary));
        self.pointer(PointerAction::Release(PointerButton::Primary));
        self.settle();
    }

    /// Drags with the left button from `from` to `to`.
    pub fn drag(&mut self, from: Vec2, to: Vec2) {
        self.move_to(from);
        self.pointer(PointerAction::Press(PointerButton::Primary));
        for step in 1..=DRAG_STEPS {
            self.move_to(from.lerp(to, step as f32 / DRAG_STEPS as f32));
        }
        self.pointer(PointerAction::Release(PointerButton::Primary));
        self.settle();
    }

    /// Turns the mouse wheel by `lines` at `at`; positive scrolls up.
    pub fn scroll(&mut self, at: Vec2, lines: f32) {
        self.move_to(at);
        self.pointer(PointerAction::Scroll {
            unit: MouseScrollUnit::Line,
            x: 0.0,
            y: lines,
            phase: TouchPhase::Moved,
        });
        self.settle();
    }

    /// Presses and releases `key`, holding `modifier` if given.
    pub fn key(&mut self, modifier: Option<KeyCode>, key: KeyCode) {
        let keys: Vec<KeyCode> = modifier.into_iter().chain([key]).collect();
        for &code in &keys {
            self.keyboard(code, ButtonState::Pressed);
        }
        for &code in keys.iter().rev() {
            self.keyboard(code, ButtonState::Released);
        }
        self.settle();
    }

    /// Types `text` into the focused text field, one character at a time,
    /// as a keyboard would. Click the field first to focus it. Characters
    /// typed with nothing focused are ignored.
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars() {
            let key_code = typed_key_code(character);
            let logical_key = if character == ' ' {
                Key::Space
            } else {
                Key::Character(character.to_string().into())
            };
            for state in [ButtonState::Pressed, ButtonState::Released] {
                self.send_key(KeyboardInput {
                    key_code,
                    logical_key: logical_key.clone(),
                    state,
                    // Only the press produces text.
                    text: (state == ButtonState::Pressed).then(|| character.to_string().into()),
                    repeat: false,
                    window: Entity::PLACEHOLDER,
                });
                self.update();
            }
        }
        self.settle();
    }

    /// Moves the mouse to `at` without pressing anything, for example to
    /// show a tooltip.
    pub fn hover(&mut self, at: Vec2) {
        self.move_to(at);
        self.settle();
    }

    /// Lets `duration` of time pass, drawing a frame for every 1/60 s of it,
    /// then runs until idle. Unlike the window, it draws even when nothing
    /// asked for a redraw.
    pub fn wait(&mut self, duration: Duration) {
        let frames = duration.as_secs_f64() / FRAME_TIME.as_secs_f64();
        for _ in 0..frames.ceil() as u32 {
            self.update();
        }
        self.settle();
    }

    /// Answers the next file dialog with `path`, or cancels it with `None`.
    /// Answers are used in order; dialogs with no answer left are cancelled.
    pub fn answer_dialog(&mut self, path: Option<PathBuf>) {
        lock(&self.dialog_answers).push_back(path);
    }

    /// Sleeps `pace` of wall-clock time after every frame, as a window waiting
    /// for the display does. Background work such as building and shader
    /// compiling then gets as much time per frame as it would in the window.
    pub fn set_pace(&mut self, pace: Duration) {
        self.pace = pace;
    }

    /// The last frame the window would have drawn, as an sRGB RGBA8 image.
    ///
    /// Collecting it runs a few more frames, which are neither shown nor
    /// counted as drawn by the window.
    ///
    /// # Panics
    ///
    /// Panics if the frame cannot be read back.
    pub fn capture(&mut self) -> Image {
        let wanted = self.shown;
        for _ in 0..=MAX_READBACK_FRAMES {
            let found = {
                let mut log = lock(&self.frames.frames);
                while log.front().is_some_and(|(number, _)| *number < wanted) {
                    log.pop_front();
                }
                log.front()
                    .filter(|(number, _)| *number == wanted)
                    .map(|(_, data)| data.clone())
            };
            if let Some(data) = found {
                return Image::new(
                    Extent3d {
                        width: self.size.x,
                        height: self.size.y,
                        depth_or_array_layers: 1,
                    },
                    TextureDimension::D2,
                    data,
                    TextureFormat::Rgba8UnormSrgb,
                    RenderAssetUsages::default(),
                );
            }
            self.run_frame();
        }
        panic!("frame {wanted} was not read back");
    }

    /// Saves the last frame drawn to `path` as a PNG.
    ///
    /// # Errors
    ///
    /// Fails if the file cannot be written.
    ///
    /// # Panics
    ///
    /// Panics as [`capture`](Self::capture) does.
    pub fn screenshot(&mut self, path: &Path) -> Result<(), String> {
        let image = self.capture();
        let image = image
            .try_into_dynamic()
            .map_err(|error| format!("cannot convert the frame: {error}"))?;
        image
            .save(path)
            .map_err(|error| format!("cannot save {}: {error}", path.display()))
    }

    /// Runs frames as the window would until nothing asks for another: at
    /// least one, then one more after each frame that asked for a redraw.
    ///
    /// With wind or auto-rotate on, the scene asks for every frame, so this
    /// gives up waiting and returns after two seconds of frames.
    ///
    /// # Panics
    ///
    /// Panics if the studio is still asking for frames after two minutes of
    /// wall-clock time.
    pub fn settle(&mut self) {
        let deadline = Instant::now() + LIMIT;
        let mut frames = 0u32;
        loop {
            let asked = self.update();
            frames += 1;
            let continuous = self
                .app
                .world()
                .resource::<crate::scene::ViewportActivity>()
                .continuous;
            // The window sleeps until a file operation wakes it on finishing;
            // check back every few milliseconds instead.
            let file_work = crate::export::file_ops_pending(self.app.world_mut());
            if file_work && !asked {
                std::thread::sleep(FILE_WORK_POLL);
            } else if !asked || (continuous && frames >= 120) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "the studio was still asking for frames after {LIMIT:?}"
            );
        }
    }

    /// Runs one frame the window would draw, and returns whether it asked for another.
    fn update(&mut self) -> bool {
        let asked = self.run_frame();
        self.shown = self.drawn;
        asked
    }

    /// Runs one frame, and returns whether it asked for another.
    fn run_frame(&mut self) -> bool {
        self.app.update();
        self.drawn += 1;
        if !self.pace.is_zero() {
            std::thread::sleep(self.pace);
        }
        let redraws = self.app.world().resource::<Messages<RequestRedraw>>();
        redraws.iter_current_update_messages().next().is_some()
    }

    /// Moves the mouse to `to` and runs one frame.
    fn move_to(&mut self, to: Vec2) {
        let delta = to - self.cursor;
        self.cursor = to;
        self.pointer(PointerAction::Move { delta });
    }

    /// Sends `action` from the mouse at its current position and runs one frame.
    fn pointer(&mut self, action: PointerAction) {
        let location = Location {
            target: NormalizedRenderTarget::Image(self.image.clone().into()),
            position: self.cursor,
        };
        self.app
            .world_mut()
            .write_message(PointerInput::new(PointerId::Mouse, location, action));
        self.update();
    }

    /// Sends a key press or release and runs one frame.
    fn keyboard(&mut self, key_code: KeyCode, state: ButtonState) {
        self.send_key(KeyboardInput {
            key_code,
            logical_key: logical_key(key_code),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        self.update();
    }

    /// Sends `input` as the keyboard would, and to the focused UI element,
    /// such as a text field. Without a window, Bevy delivers keys to no
    /// element, so this does it in its place.
    fn send_key(&mut self, input: KeyboardInput) {
        let world = self.app.world_mut();
        world.write_message(input.clone());
        if let Some(focused) = world.resource::<InputFocus>().get() {
            world.trigger(FocusedInput::new(focused, input, Entity::PLACEHOLDER));
        }
    }
}

/// Focuses the focusable element under a press, such as a text field, as
/// Bevy does in a window. Without a window Bevy focuses nothing on a press.
/// Pressing outside every focusable element keeps the focus where it was.
fn click_to_focus(press: On<PointerPress>, mut commands: Commands) {
    if press.entity == press.original_event_target() {
        commands.trigger(AcquireFocus {
            focused_entity: press.entity,
            window: Entity::PLACEHOLDER,
        });
    }
}

/// Locks `mutex`, recovering the data if a panic poisoned it.
pub fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Starts copying the window image back to the CPU every frame.
fn spawn_readback(mut commands: Commands, image: Res<UiImage>) {
    commands
        .spawn(Readback::texture(image.0.clone()))
        .observe(store_frame);
}

/// Stores each window frame read back, numbered in order.
fn store_frame(
    event: On<ReadbackComplete>,
    log: Res<FrameLog>,
    image: Res<UiImage>,
    images: Res<Assets<Image>>,
) {
    let mut delivered = lock(&log.delivered);
    *delivered += 1;
    let Some(size) = images.get(&image.0).map(Image::size) else {
        return;
    };
    if let Some(data) = unpad_rows(&event.data, size.x, size.y) {
        let mut frames = lock(&log.frames);
        frames.push_back((*delivered, data));
        // Only the newest frames can still be asked for.
        while frames.len() > KEPT_FRAMES {
            frames.pop_front();
        }
    }
}

/// The centre of a node in window pixels, or `None` if it is hidden, empty,
/// or clipped at its centre.
fn shown_center(
    node: &ComputedNode,
    transform: &UiGlobalTransform,
    visible: &InheritedVisibility,
    clip: Option<&CalculatedClip>,
) -> Option<Vec2> {
    let center = transform.translation;
    let clipped = clip.is_some_and(|clip| !clip.contains_point(center));
    (visible.get() && node.size().min_element() > 0.0 && !clipped)
        .then(|| center * node.inverse_scale_factor())
}

/// Points the UI camera at the [`UiImage`] instead of a window.
fn retarget_ui_camera(
    mut commands: Commands,
    image: Res<UiImage>,
    cameras: Query<Entity, With<IsDefaultUiCamera>>,
) {
    for camera in &cameras {
        commands
            .entity(camera)
            .insert(RenderTarget::Image(image.0.clone().into()));
    }
}

/// The logical key for the physical keys scripts can press.
fn logical_key(code: KeyCode) -> Key {
    match code {
        KeyCode::Escape => Key::Escape,
        KeyCode::Enter => Key::Enter,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ControlLeft => Key::Control,
        KeyCode::SuperLeft => Key::Super,
        _ => key_letter(code)
            .map(|letter| Key::Character(letter.to_ascii_lowercase().to_string().into()))
            .unwrap_or(Key::Unidentified(NativeKey::Unidentified)),
    }
}

/// The physical key that types `character` on a US layout, or
/// [`KeyCode::Unidentified`] for characters without one of their own.
fn typed_key_code(character: char) -> KeyCode {
    use bevy::input::keyboard::NativeKeyCode;
    const DIGITS: [KeyCode; 10] = [
        KeyCode::Digit0,
        KeyCode::Digit1,
        KeyCode::Digit2,
        KeyCode::Digit3,
        KeyCode::Digit4,
        KeyCode::Digit5,
        KeyCode::Digit6,
        KeyCode::Digit7,
        KeyCode::Digit8,
        KeyCode::Digit9,
    ];
    match character {
        'a'..='z' | 'A'..='Z' => LETTERS[(character.to_ascii_uppercase() as u8 - b'A') as usize],
        '0'..='9' => DIGITS[(character as u8 - b'0') as usize],
        ' ' => KeyCode::Space,
        '.' => KeyCode::Period,
        '-' => KeyCode::Minus,
        '_' => KeyCode::Minus,
        _ => KeyCode::Unidentified(NativeKeyCode::Unidentified),
    }
}

/// The letter a letter key types, in upper case.
fn key_letter(code: KeyCode) -> Option<char> {
    let index = LETTERS.iter().position(|&letter| letter == code)?;
    Some((b'A' + index as u8) as char)
}

/// The letter keys, A to Z.
#[rustfmt::skip]
const LETTERS: [KeyCode; 26] = [
        KeyCode::KeyA, KeyCode::KeyB, KeyCode::KeyC, KeyCode::KeyD, KeyCode::KeyE,
        KeyCode::KeyF, KeyCode::KeyG, KeyCode::KeyH, KeyCode::KeyI, KeyCode::KeyJ,
        KeyCode::KeyK, KeyCode::KeyL, KeyCode::KeyM, KeyCode::KeyN, KeyCode::KeyO,
        KeyCode::KeyP, KeyCode::KeyQ, KeyCode::KeyR, KeyCode::KeyS, KeyCode::KeyT,
        KeyCode::KeyU, KeyCode::KeyV, KeyCode::KeyW, KeyCode::KeyX, KeyCode::KeyY,
        KeyCode::KeyZ,
];

/// Parses a script key such as `Escape`, `q` or `cmd+q`.
fn parse_key(text: &str) -> Result<(Option<KeyCode>, KeyCode), String> {
    let (modifier, name) = match text.split_once('+') {
        Some(("ctrl", name)) => (Some(KeyCode::ControlLeft), name),
        Some(("cmd", name)) => (Some(KeyCode::SuperLeft), name),
        Some((other, _)) => return Err(format!("unknown modifier {other:?}")),
        None => (None, text),
    };
    let key = match name {
        "Escape" => KeyCode::Escape,
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        _ => {
            let mut chars = name.chars();
            let (Some(letter), None) = (chars.next(), chars.next()) else {
                return Err(format!("unknown key {name:?}"));
            };
            let index = (letter.to_ascii_uppercase() as u32).wrapping_sub('A' as u32);
            *LETTERS
                .get(index as usize)
                .ok_or_else(|| format!("unknown key {name:?}"))?
        }
    };
    Ok((modifier, key))
}

/// Runs `procgena --headless <script>` when those are the arguments, returning
/// the exit code. Returns `None` for any other arguments.
pub fn run_from_args() -> Option<i32> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() != Some("--headless") {
        return None;
    }
    let Some(path) = args.next() else {
        eprintln!("usage: procgena --headless <script | ->");
        return Some(2);
    };
    let script = if path == "-" {
        std::io::read_to_string(std::io::stdin())
    } else {
        std::fs::read_to_string(&path)
    };
    let script = match script {
        Ok(script) => script,
        Err(error) => {
            eprintln!("cannot read {path}: {error}");
            return Some(2);
        }
    };
    match run_script(&script) {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("{error}");
            Some(1)
        }
    }
}

/// Follows `script`, described in the [module documentation](self).
///
/// # Errors
///
/// Fails on the first line that cannot be parsed or followed, naming it.
pub fn run_script(script: &str) -> Result<(), String> {
    let lines: Vec<(usize, &str)> = script
        .lines()
        .enumerate()
        .map(|(i, line)| (i + 1, line.trim()))
        .filter(|(_, line)| !line.is_empty() && !line.starts_with('#'))
        .collect();

    // The size must come first, since the studio starts at that size.
    let mut size = DEFAULT_SIZE;
    let mut rest = lines.as_slice();
    if let Some(((number, line), after)) = rest.split_first()
        && let Some(args) = line.strip_prefix("size ")
    {
        let [w, h] = numbers(args).map_err(|e| format!("line {number}: {e}"))?;
        size = UVec2::new(w as u32, h as u32);
        rest = after;
    }

    let mut studio = HeadlessStudio::new(size);
    for (number, line) in rest {
        run_line(&mut studio, line).map_err(|error| format!("line {number} ({line}): {error}"))?;
    }
    Ok(())
}

/// Follows one script line.
fn run_line(studio: &mut HeadlessStudio, line: &str) -> Result<(), String> {
    let (command, args) = line.split_once(' ').unwrap_or((line, ""));
    let args = args.trim();
    match command {
        "click" => studio.click(args)?,
        "click-at" => {
            let [x, y] = numbers(args)?;
            studio.click_at(Vec2::new(x, y));
        }
        "drag" => {
            let [x1, y1, x2, y2] = numbers(args)?;
            studio.drag(Vec2::new(x1, y1), Vec2::new(x2, y2));
        }
        "scroll" => {
            let [x, y, lines] = numbers(args)?;
            studio.scroll(Vec2::new(x, y), lines);
        }
        "key" => {
            let (modifier, key) = parse_key(args)?;
            studio.key(modifier, key);
        }
        "type" => studio.type_text(args),
        "select" => studio.select(args)?,
        "command" => studio.command(args)?,
        "hover" => {
            let [x, y] = numbers(args)?;
            studio.hover(Vec2::new(x, y));
        }
        "wait" => {
            let [seconds] = numbers(args)?;
            studio.wait(Duration::from_secs_f32(seconds.max(0.0)));
        }
        "choose" => studio.answer_dialog(Some(PathBuf::from(args))),
        "cancel-dialog" => studio.answer_dialog(None),
        "pace" => {
            let [millis] = numbers(args)?;
            studio.set_pace(Duration::from_secs_f32(millis.max(0.0) / 1000.0));
        }
        "shot" => studio.screenshot(Path::new(args))?,
        "dump" => {
            for text in studio.texts() {
                println!(
                    "{:?} at {:.0},{:.0} size {:.0}x{:.0}",
                    text.text, text.center.x, text.center.y, text.size.x, text.size.y
                );
            }
        }
        "size" => return Err("size must be the first command".into()),
        _ => return Err(format!("unknown command {command:?}")),
    }
    Ok(())
}

/// Parses exactly `N` space-separated numbers.
fn numbers<const N: usize>(args: &str) -> Result<[f32; N], String> {
    let parsed: Vec<f32> = args
        .split_whitespace()
        .map(|word| word.parse().map_err(|_| format!("not a number: {word:?}")))
        .collect::<Result<_, _>>()?;
    parsed
        .try_into()
        .map_err(|_| format!("expected {N} numbers"))
}

#[cfg(test)]
mod tests {
    //! These start the whole studio on the GPU, so they are ignored by default.
    //! Run them with `cargo test -p procgena -- --ignored`.

    use super::*;
    use crate::studio::documents::{Document, Selection};
    use crate::ui::toast::{ShowToast, ToastKind};

    #[test]
    fn script_keys_are_parsed() {
        assert_eq!(parse_key("Escape"), Ok((None, KeyCode::Escape)));
        assert_eq!(
            parse_key("cmd+q"),
            Ok((Some(KeyCode::SuperLeft), KeyCode::KeyQ))
        );
        assert_eq!(
            parse_key("ctrl+Z"),
            Ok((Some(KeyCode::ControlLeft), KeyCode::KeyZ))
        );
        assert!(parse_key("alt+q").is_err());
        assert!(parse_key("F13").is_err());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_opening_frame_shows_the_tree_beside_the_panel() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let frame = studio.capture();
        let leafy = |x: u32, y: u32| {
            let Ok(color) = frame.get_color_at(x, y) else {
                return false;
            };
            let [r, g, b, _] = color.to_srgba().to_u8_array();
            g > r.saturating_add(10) && g > b.saturating_add(10)
        };
        // Foliage covers part of the 3D view, between the Library and the
        // Inspector.
        let view_pixels = (260..1076)
            .step_by(4)
            .flat_map(|x| (0..900).step_by(4).map(move |y| (x, y)));
        let leaves = view_pixels.filter(|&(x, y)| leafy(x, y)).count();
        assert!(
            leaves > 500,
            "only {leaves} sampled pixels look like leaves"
        );
        assert!(studio.find("Trees").is_ok() && studio.find("Reset Camera").is_ok());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn screenshots_show_the_last_frame_the_window_drew() {
        let mut studio = HeadlessStudio::new(UVec2::new(400, 300));
        let red_at_corner = |frame: &Image| {
            let color = frame.get_color_at(10, 10).unwrap().to_srgba();
            color.red > 0.9 && color.green < 0.1 && color.blue < 0.1
        };

        // A change the window has not drawn yet is not in the screenshot.
        studio.app_mut().world_mut().spawn((
            Node {
                position_type: PositionType::Absolute,
                width: px(40),
                height: px(40),
                ..default()
            },
            BackgroundColor(Color::srgb(1.0, 0.0, 0.0)),
            GlobalZIndex(1000),
        ));
        assert!(!red_at_corner(&studio.capture()));

        // Once drawn it is, and capturing again shows that same frame.
        studio.settle();
        let first = studio.capture();
        assert!(red_at_corner(&first));
        assert_eq!(studio.capture().data, first.data);
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn opening_a_preset_from_the_library_shows_its_editor_and_subject() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        assert!(studio.find("Detail").is_ok());
        studio.click("Stones").unwrap();
        studio.click("Boulder").unwrap();
        assert_eq!(selected(&mut studio).kind, "stone");
        // The tree's detail control is gone and the overlay describes the rock.
        assert!(studio.find("Detail").is_err());
        let texts = studio.texts();
        assert!(
            texts
                .iter()
                .any(|t| t.text.contains("vertices") && !t.text.starts_with("LOD")),
            "no rock size in {texts:?}"
        );
    }

    /// The selected document.
    fn selected(studio: &mut HeadlessStudio) -> Document {
        let world = studio.app_mut().world();
        let document = world.resource::<Selection>().document.unwrap();
        world.get::<Document>(document).unwrap().clone()
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn two_trees_keep_their_own_settings_and_closing_one_selects_the_other() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let oak = studio.panel_value::<procgena_tree::TreeOptions>();

        // A second tree opens beside the first, with its own settings.
        studio.click("Aspen Small").unwrap();
        let aspen = studio.panel_value::<procgena_tree::TreeOptions>();
        assert_eq!(aspen, procgena_tree::TreePreset::aspen_small().tree);
        studio.click("Random Seed").unwrap();
        let reseeded = studio.panel_value::<procgena_tree::TreeOptions>();
        assert_ne!(reseeded.default_seed, aspen.default_seed);

        // The first tree is as it was; opening its preset again selects it.
        studio.click("Oak Medium").unwrap();
        assert_eq!(selected(&mut studio).key, "oak_medium");
        assert_eq!(studio.panel_value::<procgena_tree::TreeOptions>(), oak);
        studio.command("next-document").unwrap();
        assert_eq!(studio.panel_value::<procgena_tree::TreeOptions>(), reseeded);

        // The Outline lists them in the order they were opened.
        let oak = studio.find("Oak Medium · Trees").unwrap();
        let aspen = studio.find("Aspen Small · Trees").unwrap();
        assert!(oak.y < aspen.y, "{oak} then {aspen}");

        // Closing the aspen selects the oak; closing the oak too leaves the
        // view empty.
        studio.command("close-document").unwrap();
        assert_eq!(selected(&mut studio).key, "oak_medium");
        assert!(studio.find("Aspen Small · Trees").is_err());
        studio.command("close-document").unwrap();
        let world = studio.app_mut().world_mut();
        assert_eq!(world.resource::<Selection>().document, None);
        let trees = world
            .query::<&procgena_tree_bevy::TreeParams>()
            .iter(world)
            .count();
        assert_eq!(trees, 0, "the tree is gone from the view");
        assert!(studio.find("Open a preset from the Library").is_ok());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn scripts_select_documents_and_run_commands() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.select("ecotope/pine_forest_floor#ground").unwrap();
        assert_eq!(selected(&mut studio).kind, "ecotope");
        let selection = studio.app_mut().world().resource::<Selection>().clone();
        assert_eq!(selection.part.as_deref(), Some("ground"));
        assert!(studio.select("ecotope/no_such_preset").is_err());
        assert!(studio.select("tree").is_err());
        assert!(studio.command("no-such-command").is_err());
        studio.select("tree/oak_medium").unwrap();
        assert_eq!(selected(&mut studio).kind, "tree");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn typing_into_a_text_field_renames_a_scatter() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.select("ecotope/pine_forest_floor").unwrap();
        studio.click("+ Add Scatter").unwrap();

        // Clicking the new Scatter's key field focuses it; select its text
        // and type over it. The Scatter list shows the key too, so find the
        // field itself.
        let world = studio.app_mut().world_mut();
        let mut fields = world.query::<(Entity, &EditableText)>();
        let field = fields
            .iter(world)
            .find(|(_, text)| text.value() == "trees")
            .map(|(entity, _)| entity)
            .unwrap();
        let at = studio.center_of(field).unwrap();
        studio.click_at(at);
        let command = if cfg!(target_os = "macos") {
            KeyCode::SuperLeft
        } else {
            KeyCode::ControlLeft
        };
        let focus = studio.app_mut().world().resource::<InputFocus>().get();
        assert_eq!(focus, Some(field), "the key field has focus");
        studio.key(Some(command), KeyCode::KeyA);
        studio.type_text("young_pines 2");
        assert!(studio.find("young_pines 2").is_ok(), "{:?}", studio.texts());
        studio.key(None, KeyCode::Backspace);
        studio.key(None, KeyCode::Backspace);
        studio.click("Rename").unwrap();
        let world = studio.app_mut().world_mut();
        let mut editors = world.query_filtered::<
            &crate::ecotope::editor::EcotopeEditor,
            Without<crate::studio::Dormant>,
        >();
        let ecotope = editors.single(world).unwrap().ecotope().clone();
        let keys: Vec<String> = ecotope.scatters.keys().map(|key| key.to_string()).collect();
        assert!(keys.iter().any(|key| key == "young_pines"), "{keys:?}");
        assert!(!keys.iter().any(|key| key == "trees"), "{keys:?}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn escape_asks_before_quitting_and_cancel_keeps_the_studio() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        studio.key(None, KeyCode::Escape);
        assert!(studio.find("Quit procgena?").is_ok());
        studio.click("Cancel").unwrap();
        assert!(studio.find("Quit procgena?").is_err());
        let exits = studio.app_mut().world().resource::<Messages<AppExit>>();
        assert!(exits.is_empty());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn each_subject_keeps_its_camera_and_the_detail_picker_drives_the_overlay() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let camera = |studio: &mut HeadlessStudio| {
            let world = studio.app_mut().world_mut();
            let mut cameras = world
                .query_filtered::<&crate::scene::OrbitCamera, With<crate::scene::ViewportCamera>>();
            *cameras.single(world).unwrap()
        };
        let continuous = |studio: &mut HeadlessStudio| {
            let world = studio.app_mut().world();
            world
                .resource::<crate::scene::ViewportActivity>()
                .continuous
        };

        // Turn the tree's camera, visit the rock, and come back to the same view.
        studio.drag(Vec2::new(500.0, 400.0), Vec2::new(700.0, 350.0));
        let tree_view = camera(&mut studio);
        studio.click("Stones").unwrap();
        studio.click("Boulder").unwrap();
        assert_ne!(camera(&mut studio), tree_view);
        studio.click("Oak Medium · Trees").unwrap();
        assert_eq!(camera(&mut studio), tree_view);

        // The game default draws no finer than level 1; picking a level
        // shows that level's size in the overlay.
        assert!(
            studio.shows_text_starting("LOD 1 ·"),
            "{:?}",
            studio.texts()
        );
        studio.click("Game default").unwrap();
        studio.click("2 · Low").unwrap();
        assert!(
            studio.shows_text_starting("LOD 2 ·"),
            "{:?}",
            studio.texts()
        );

        // Wind animates every frame. With it off, an idle studio asks for no
        // more frames, so the window sleeps. The switch sits right of its
        // label in the Environment settings over the view.
        assert!(continuous(&mut studio), "wind starts on");
        studio.click("Environment").unwrap();
        let wind = studio.find("Wind").unwrap();
        studio.click_at(Vec2::new(wind.x + 130.0, wind.y));
        assert!(!continuous(&mut studio));
        assert!(!studio.update());
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn dragging_the_divider_resizes_the_panel_within_its_limits() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let before = studio.find("Random Seed").unwrap();
        // The divider sits just left of the 360-pixel panel.
        let divider = Vec2::new(1440.0 - 360.0 - 2.0, 450.0);
        studio.drag(divider, divider - Vec2::new(100.0, 0.0));
        let wider = studio.find("Random Seed").unwrap();
        assert!(
            (before.x - wider.x - 100.0).abs() < 1.0,
            "{before} -> {wider}"
        );
        // The panel stops growing at 640 pixels.
        studio.drag(
            divider - Vec2::new(100.0, 0.0),
            divider - Vec2::new(900.0, 0.0),
        );
        let widest = studio.find("Random Seed").unwrap();
        assert!(
            (before.x - widest.x - 280.0).abs() < 1.0,
            "{before} -> {widest}"
        );
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn the_wheel_scrolls_settings_taller_than_the_window() {
        // A tree's settings are far taller than this window. The settings
        // scroll under the Inspector's title, so follow one well below it.
        let mut studio = HeadlessStudio::new(UVec2::new(1440, 400));
        let before = studio.find("Branches").unwrap();
        studio.scroll(Vec2::new(1250.0, 300.0), -3.0);
        let after = studio.find("Branches").unwrap();
        assert!(after.y < before.y - 40.0, "{before} -> {after}");
    }

    #[test]
    #[ignore = "needs a GPU"]
    fn success_messages_show_then_hide_and_errors_stay() {
        let mut studio = HeadlessStudio::new(DEFAULT_SIZE);
        let world = studio.app_mut().world_mut();
        world.trigger(ShowToast {
            kind: ToastKind::Success,
            message: "Saved tree.json".into(),
        });
        world.trigger(ShowToast {
            kind: ToastKind::Error,
            message: "Cannot save".into(),
        });
        studio.settle();
        assert!(studio.find("Saved tree.json").is_ok());
        studio.wait(Duration::from_secs(5));
        assert!(studio.find("Saved tree.json").is_err());
        assert!(studio.find("Cannot save").is_ok());
    }
}
