//! Checks that the dither shader module compiles and validates when used
//! from a fragment and a compute shader, and that its dither levels match
//! the Rust `LodRange::dither_level` at every kind of distance.

use std::collections::HashMap;

use procgena_core::{Lod, LodLevel, LodRange};
use procgena_core_bevy::DITHER_WESL;
use wesl::eval::Inputs;
use wesl::resolver::VirtualResolver;
use wesl::{CompileResult, Compiler};

/// Compiles `main` with the dither module importable as `package::dither`.
fn compile(main: &str) -> CompileResult {
    let mut resolver = VirtualResolver::new();
    resolver.add_module("package::dither".parse().unwrap(), DITHER_WESL.into());
    resolver.add_module("package::main".parse().unwrap(), main.to_string().into());
    Compiler::default()
        .with_resolver(resolver)
        .compile_module(&"package::main".parse().unwrap())
        .unwrap_or_else(|e| panic!("WESL compile failed: {e}"))
}

/// A renderer's use of the module: a compute pass sorting copies into the
/// levels they show at, and a fragment shader discarding hidden pixels.
const RENDERER_MAIN: &str = r#"
import package::dither::{dither_level, dither_discard, is_drawn, switch_distance};

@group(0) @binding(0) var<storage, read> ranges: array<vec4<f32>, 4>;
@group(0) @binding(1) var<storage, read_write> levels: array<i32, 4>;

@compute @workgroup_size(1)
fn sort(@builtin(global_invocation_id) id: vec3<u32>) {
    let distance = f32(id.x);
    for (var lod = 0u; lod < 4u; lod++) {
        let dither = dither_level(ranges[lod], distance);
        if is_drawn(dither) && distance >= switch_distance(ranges[lod]) {
            levels[lod] = dither;
        }
    }
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>, @location(0) @interpolate(flat) dither: i32) -> @location(0) vec4<f32> {
    dither_discard(position, dither);
    return vec4<f32>(1.0);
}
"#;

#[test]
fn shader_passes_naga_validation() {
    let wgsl = compile(RENDERER_MAIN).to_string();
    let module = naga::front::wgsl::parse_str(&wgsl)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::default(),
    )
    .validate(&module)
    .unwrap_or_else(|e| panic!("naga validation failed: {e:?}"));
}

/// The shader's dither level for `range` at each of up to four `distances`,
/// run in WESL's interpreter without a GPU.
fn shader_levels(range: &LodRange, distances: &[f32]) -> Vec<i32> {
    assert!((1..=4).contains(&distances.len()));
    let v = range.to_vec4();
    // The result is a vec4, so repeat the last distance to fill it.
    let calls: Vec<String> = (0..4)
        .map(|i| distances[i.min(distances.len() - 1)])
        .map(|d| format!("dither_level(range, {d:?})"))
        .collect();
    let main = format!(
        r#"
import package::dither::dither_level;

@fragment
fn main() -> @location(0) vec4<i32> {{
    let range = vec4<f32>({:?}, {:?}, {:?}, {:?});
    return vec4<i32>({});
}}
"#,
        v.x,
        v.y,
        v.z,
        v.w,
        calls.join(", "),
    );
    let compiled = compile(&main);
    let result = compiled
        .exec("main", Inputs::default(), HashMap::new(), HashMap::new())
        .unwrap_or_else(|e| panic!("WESL exec failed: {e}"));
    let bytes = result.inst.expect("a return value").to_buffer().unwrap();
    (0..distances.len())
        .map(|i| i32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap()))
        .collect()
}

#[test]
fn shader_dither_levels_match_rust() {
    let lod = Lod::new(
        1.0,
        vec![
            LodLevel::new(0.0, 0.0, ()),
            LodLevel::new(10.0, 0.2, ()),
            LodLevel::new(20.0, 0.1, ()),
            LodLevel::new(40.0, 0.0, ()),
        ],
    )
    .with_cull(80.0);
    let mut distances = vec![
        0.0, 7.9, 8.0, 8.125, 9.0, 10.0, 11.0, 11.99, 12.0, 15.0, 18.0, 18.125, 20.0, 21.0, 21.99,
        22.0, 39.99, 40.0, 79.99, 80.0, 1e6,
    ];
    // Exact halves of a dither step, where rounding modes differ.
    distances.extend([8.0 + 4.0 * 0.5 / 16.0, 18.0 + 4.0 * 2.5 / 16.0]);

    for (i, range) in lod.ranges(1.0).iter().enumerate() {
        let rust: Vec<i32> = distances.iter().map(|&d| range.dither_level(d)).collect();
        for chunk in (0..distances.len()).collect::<Vec<_>>().chunks(4) {
            let picked: Vec<f32> = chunk.iter().map(|&j| distances[j]).collect();
            let shader = shader_levels(range, &picked);
            let expected: Vec<i32> = chunk.iter().map(|&j| rust[j]).collect();
            assert_eq!(&shader[..picked.len()], expected, "level {i} at {picked:?}");
        }
    }
}
