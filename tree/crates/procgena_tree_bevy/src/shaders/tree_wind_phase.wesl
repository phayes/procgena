// How a tree's wind phase shifts its sway, shared by every shader that sways
// a tree or something attached to it.

/// How far apart in timing two trees on the same spot can sway, as a share of
/// one sway cycle: a tree's wind phase of 0 to 1 shifts it by up to this much.
const WIND_PHASE_SPREAD: f32 = 0.15;

/// The timing shift, in radians, for a tree with wind phase `wind_phase`
/// (0 to 1). Pass it as `shift` to `procgena_tree::wind::wind_offset_shifted`.
/// Use the same phase for the tree and everything attached to it.
fn wind_phase_shift(wind_phase: f32) -> f32 {
    return 6.283185307 * WIND_PHASE_SPREAD * wind_phase;
}
