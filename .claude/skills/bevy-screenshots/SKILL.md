---
name: bevy-screenshots
description: Take screenshots of procgena's Bevy viewers and studio without opening a window, to check shaders, textures, LOD and layout by eye. Use whenever you need to see what a generator or the studio renders, before adding a `--headless` mode to a viewer that lacks one, and when a screenshot comes out blank, black or missing pieces.
---

# Headless Bevy screenshots

Never open a window to look at a change. Every viewer below can render into
an offscreen image and save a PNG, then exit. A GPU is still needed (Metal
on this Mac); no display or focus is.

Save screenshots in your scratchpad directory, not the repository, and look
at them with the Read tool. Run them in the background when the build is
cold; a warm run takes about 10 s.

## Viewers that support it

| Crate | Command (after `cargo run -p <crate> --example viewer --`) | Textures from |
|---|---|---|
| `procgena_deadwood_bevy` | `--headless --screenshot <png>`; also `--preset <snake_name>`, `--scatter <kindling\|twig_litter>`, `--distance <m>` (for checking each detail level), `--closeup`, `--orbit <deg>`, `--flat`, `--no-textures` | `deadwood/assets/textures` |
| `procgena_clump_bevy` | `--headless --screenshot <png>`; also `--eye x y z --target x y z` | `clump`, `stone`, `tree` assets |
| `procgena_stone_bevy` | `--headless --screenshot <png>`; also `--eye`, `--target`, `--resolution w h` | `stone/assets/textures` |
| `procgena_tree_bevy` | `--headless --screenshot <png>`; also `--preset`, `--eye`, `--target`, `--lod`, `--resolution` | `tree/assets/textures` |
| `procgena` (studio) | `cargo run -p procgena -- --headless <script>` (debug builds); script commands such as `click TEXT`, `wait S`, `shot PATH`, `dump`. See `procgena/src/headless.rs`. | all |

`procgena_mushroom_bevy`'s viewer has `--screenshot` but no `--headless`
yet; add it with the recipe below before using it.

Each example's opening doc comment lists every option; read it first.
Download missing textures with `uv run --project tools textures download
<project>`; viewers fall back to vertex colours when files are missing.

## Adding `--headless` to a viewer

Follow `deadwood/crates/procgena_deadwood_bevy/examples/viewer.rs`:

1. **Plugins.** With `--headless`, set `WindowPlugin { primary_window: None,
   exit_condition: ExitCondition::DontExit, .. }`, disable `WinitPlugin`, and
   add `ScheduleRunnerPlugin::run_loop(Duration::from_secs_f64(1.0 / 60.0))`,
   which drives frames instead of the window. Leave out `FreeCameraPlugin`,
   which needs a window. Require `--screenshot` with `--headless`.
2. **Render target.** In the startup system, create
   `Image::new_target_texture(1280, 720, TextureFormat::Rgba8UnormSrgb,
   None)`, add it to `Assets<Image>`, insert
   `RenderTarget::Image(handle.clone().into())` on the camera entity, and keep
   the handle in a resource.
3. **Capture.** Wait until generation has finished (no `*Generating`
   components, at least one part entity), then a settle period of frames so
   textures and shaders are ready (deadwood uses 90). Spawn
   `Screenshot::image(handle)` (or `Screenshot::primary_window()` when not
   headless), `.observe(save_to_disk(path))`, and a second observer on
   `ScreenshotCaptured` that writes `AppExit::Success`.
4. **Timeout.** If nothing is built within a time limit, log an error and
   exit with `AppExit::error()`, so a broken build can't hang a script.

## When the image is wrong

- **Blank or sky only:** the camera isn't framing the subject; check the
  viewer's camera options, or the piece failed to build (look for errors in
  the log).
- **Textured pieces missing:** materials with textures are not drawn until
  every image loads, and never if one is missing. Re-run with the viewer's
  no-textures flag to tell texture problems from mesh problems.
- **Black or garbage surfaces:** look in the log for `wgpu` or shader
  pipeline errors. A crate's GPU test (for example
  `cargo test -p procgena_deadwood_bevy --test gpu -- --include-ignored`)
  checks that every pipeline compiles.
- **Washed out:** lighting is tuned per viewer; compare against a screenshot
  taken before your change rather than judging brightness alone.

Follow the workspace rules in `CLAUDE.md`: always pass `-p <crate>`, and run
only the viewers for the crates you changed.
