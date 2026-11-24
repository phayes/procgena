//! Checks that the WESL shaders compile and reproduce the CPU generator when
//! run in the WESL interpreter.

use std::collections::HashMap;

use wesl::eval::{Context, EvalAttrs, Exec, Inputs, Instance, RefInstance, Type, ty_eval_ty};
use wesl::resolver::VirtualResolver;
use wesl::syntax::{AccessMode, AddressSpace, GlobalDeclaration};
use wesl::{CompileOptions, CompileResult, Compiler};

use glam::Vec3;
use naga::valid::{Capabilities, ValidationFlags, Validator};
use procgena_specimen::SurfaceMesh;

use super::*;
use crate::bvh::triangles_cross;
use crate::field::{ClumpField, LUMP_HEIGHT_MIN, LUMP_RADIUS_MAX, LUMP_RADIUS_MIN, SurfacePoint};
use crate::options::{ClumpOptions, Direction, MAX_DIRECTIONS};
use crate::patch::{MIN_CORNER_COS, MIN_QUALITY, Placement, border_stays_near, collapse_quality};
use crate::presets::ClumpPreset;
use crate::shell::{layer_top, limit_tilt, lowered};
use procgena_core::Rng;
use procgena_core::hash::{pcg3d, unit_float};

/// Compiles `root` with every shader module available. With `strip`, only
/// declarations reachable from entry points are kept.
fn compile_with(root: &str, extra: &[(&str, &str)], strip: bool) -> CompileResult {
    let mut resolver = VirtualResolver::new();
    for (path, source) in SHADER_MODULES.iter().chain(extra) {
        resolver.add_module(path.parse().unwrap(), source.to_string().into());
    }
    let options = CompileOptions {
        strip,
        ..Default::default()
    };
    Compiler::new_with_resolver(options, resolver)
        .compile_module(&root.parse().unwrap())
        .unwrap_or_else(|e| panic!("{root}: WESL compile failed: {e}"))
}

/// Compiles a test module that can import every shader module.
fn compile_main(main: &str) -> CompileResult {
    compile_with("package::main", &[("package::main", main)], true)
}

/// A compiled module with resources, run one invocation at a time in the WESL interpreter.
struct Interpreter {
    compiled: CompileResult,
    bindings: HashMap<(u32, u32), RefInstance>,
}

impl Interpreter {
    fn new(compiled: CompileResult) -> Self {
        Self {
            compiled,
            bindings: HashMap::new(),
        }
    }

    /// Returns the declared type, address space and access mode of the
    /// variable at `group`, `binding`.
    fn binding_type(&self, group: u32, binding: u32) -> (Type, AddressSpace, AccessMode) {
        let mut ctx = Context::new(&self.compiled.syntax);
        self.compiled.syntax.exec(&mut ctx).unwrap();
        for decl in &self.compiled.syntax.global_declarations {
            if let GlobalDeclaration::Declaration(d) = decl.node()
                && d.attr_group_binding(&mut ctx).ok() == Some((group, binding))
            {
                let ty = ty_eval_ty(d.ty.as_ref().unwrap(), &mut ctx).unwrap();
                let (space, access) = match d.kind {
                    wesl::syntax::DeclarationKind::Var(Some((space, access))) => (space, access),
                    _ => panic!("binding {group}:{binding} is not a buffer"),
                };
                // Uniforms are read-only; storage defaults to read-only too.
                let access = match space {
                    AddressSpace::Uniform => AccessMode::Read,
                    _ => access.unwrap_or(AccessMode::Read),
                };
                return (ty, space, access);
            }
        }
        panic!("no binding {group}:{binding}");
    }

    /// Binds `bytes` as the variable at `group`, `binding`.
    fn bind(&mut self, group: u32, binding: u32, bytes: &[u8]) {
        let (ty, space, access) = self.binding_type(group, binding);
        let inst = Instance::from_buffer(bytes, &ty).unwrap_or_else(|| {
            panic!(
                "binding {group}:{binding}: {} bytes do not fit {ty:?}",
                bytes.len()
            )
        });
        self.bindings
            .insert((group, binding), RefInstance::new(inst, space, access));
    }

    /// Returns the current contents of a bound buffer.
    fn read(&self, group: u32, binding: u32) -> Vec<u8> {
        self.bindings[&(group, binding)]
            .read()
            .unwrap()
            .to_buffer()
            .unwrap()
    }

    /// Runs `entry` for items `0..items`, as a dispatch of 64-wide workgroups would.
    fn dispatch(&self, entry: &str, items: u32) {
        let vec3 = |v: [u32; 3]| {
            let bytes: Vec<u8> = v.iter().flat_map(|w| w.to_le_bytes()).collect();
            Instance::from_buffer(&bytes, &Type::Vec(3, Box::new(Type::U32))).unwrap()
        };
        let workgroups = items.div_ceil(64);
        for i in 0..items {
            let mut inputs = Inputs::default();
            inputs
                .builtins
                .insert("global_invocation_id".into(), vec3([i, 0, 0]));
            inputs
                .builtins
                .insert("num_workgroups".into(), vec3([workgroups, 1, 1]));
            self.compiled
                .exec(entry, inputs, self.bindings.clone(), HashMap::new())
                .unwrap_or_else(|e| panic!("{entry}[{i}]: {e}"));
        }
    }

    /// Runs a single-invocation entry point without inputs.
    fn run(&self, entry: &str) {
        self.compiled
            .exec(
                entry,
                Inputs::default(),
                self.bindings.clone(),
                HashMap::new(),
            )
            .unwrap_or_else(|e| panic!("{entry}: {e}"));
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn bytes_of(words: impl IntoIterator<Item = u32>) -> Vec<u8> {
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

/// Runs `body` in a compute entry point whose `out` array of `outputs`
/// words receives the results, and returns `out`.
fn run_main(imports: &str, body: &str, outputs: usize) -> Vec<u32> {
    let main = format!(
        r#"
{imports}
@group(1) @binding(0) var<storage, read_write> out: array<u32>;
@compute @workgroup_size(1)
fn main() {{
{body}
}}
"#
    );
    let mut interp = Interpreter::new(compile_main(&main));
    interp.bind(1, 0, &vec![0; 4 * outputs]);
    interp.run("main");
    words(&interp.read(1, 0))
}

/// Runs `body` once in a compute entry point and returns its `out` words.
///
/// The body reads `input: array<u32>` (filled from `input`), can read the
/// uniform `field: Field` (filled from `field`), and writes `outputs` words
/// to `out: array<u32>`. Inputs go through a buffer, not literals, because
/// WGSL constants can't be infinite or NaN.
fn run_kernel(
    imports: &str,
    field: &[u32; 48],
    input: &[u32],
    body: &str,
    outputs: usize,
) -> Vec<u32> {
    let main = format!(
        r#"
import package::field::Field;
{imports}
@group(1) @binding(0) var<storage, read_write> out: array<u32>;
@group(1) @binding(1) var<uniform> field: Field;
@group(1) @binding(2) var<storage, read> input: array<u32>;

fn f(i: u32) -> f32 {{ return bitcast<f32>(input[i]); }}
fn v3(i: u32) -> vec3<f32> {{ return vec3<f32>(f(i), f(i + 1u), f(i + 2u)); }}
fn put(i: u32, x: f32) {{ out[i] = bitcast<u32>(x); }}
fn put3(i: u32, v: vec3<f32>) {{ put(i, v.x); put(i + 1u, v.y); put(i + 2u, v.z); }}

@compute @workgroup_size(1)
fn main() {{
{body}
}}
"#
    );
    // Unstripped, so `field` stays bound even when the body doesn't read it.
    let compiled = compile_with("package::main", &[("package::main", &main)], false);
    let mut interp = Interpreter::new(compiled);
    interp.bind(1, 0, &vec![0; 4 * outputs]);
    interp.bind(1, 1, &bytes_of(*field));
    interp.bind(1, 2, &bytes_of(input.iter().copied()));
    interp.run("main");
    words(&interp.read(1, 0))
}

/// Pushes the bits of `v`'s components.
fn push3(words: &mut Vec<u32>, v: Vec3) {
    words.extend(v.to_array().map(f32::to_bits));
}

/// Returns a random vector with components in `[-1, 1)`.
fn random_vec(rng: &mut Rng) -> Vec3 {
    Vec3::new(
        rng.range(-1.0, 1.0),
        rng.range(-1.0, 1.0),
        rng.range(-1.0, 1.0),
    )
}

/// Returns a random unit vector.
fn random_unit(rng: &mut Rng) -> Vec3 {
    random_vec(rng).normalize_or(Vec3::Y)
}

/// Asserts that `gpu` holds exactly the bits of `cpu`.
fn assert_bits(gpu: f32, cpu: f32, what: &str) {
    assert!(
        gpu.to_bits() == cpu.to_bits(),
        "{what}: GPU {gpu:e} ({:#x}) vs CPU {cpu:e} ({:#x})",
        gpu.to_bits(),
        cpu.to_bits()
    );
}

#[test]
fn every_module_compiles_and_validates() {
    for (path, _) in SHADER_MODULES {
        let wgsl = compile_with(path, &[], false).to_string();
        let module = naga::front::wgsl::parse_str(&wgsl)
            .unwrap_or_else(|e| panic!("{path}: {}", e.emit_to_string(&wgsl)));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{path}: {e:?}"));
    }
}

/// The WGSL `build.rs` wrote is valid, and covers exactly the entry modules.
#[cfg(feature = "gpu")]
#[test]
fn build_script_compiles_every_entry_module() {
    let names: Vec<&str> = COMPILED_WGSL.iter().map(|(name, _)| *name).collect();
    assert_eq!(names, ENTRY_MODULES);
    for (name, wgsl) in COMPILED_WGSL {
        let module = naga::front::wgsl::parse_str(wgsl)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(wgsl)));
        Validator::new(ValidationFlags::all(), Capabilities::empty())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: {e:?}"));
    }
}

/// Returns the byte size and member offsets of struct `name` in `wgsl`, as
/// naga lays it out.
#[cfg(feature = "gpu")]
fn struct_layout(wgsl: &str, name: &str) -> (u32, Vec<(String, u32)>) {
    let module = naga::front::wgsl::parse_str(wgsl).unwrap();
    for (_, ty) in module.types.iter() {
        if ty.name.as_deref() == Some(name)
            && let naga::TypeInner::Struct { members, span } = &ty.inner
        {
            let offsets = members
                .iter()
                .map(|m| (m.name.clone().unwrap_or_default(), m.offset))
                .collect();
            return (*span, offsets);
        }
    }
    panic!("no struct {name}");
}

/// The generator's structs have the sizes and member offsets that the Rust
/// packing assumes.
#[cfg(feature = "gpu")]
#[test]
fn generator_structs_match_rust_packing() {
    use crate::gpu::layout_sizes;

    let wgsl = compile_with("package::batch", &[], false).to_string();
    let (request, members) = struct_layout(&wgsl, "Request");
    assert_eq!(request as usize, layout_sizes::REQUEST);
    let at = |name: &str| members.iter().find(|(n, _)| n == name).unwrap().1;
    // A 32-word header, then the transforms and the field.
    assert_eq!(at("clump_radius"), 96);
    assert_eq!(at("transform"), 128);
    assert_eq!(at("normal_transform"), 176);
    assert_eq!(at("field"), 224);
    let (batch, members) = struct_layout(&wgsl, "Batch");
    assert_eq!(batch as usize, layout_sizes::BATCH);
    assert_eq!(members.iter().find(|(n, _)| n == "regions").unwrap().1, 32);
    // `Field` is declared in its own module; imported names are mangled.
    let wgsl = compile_with("package::field", &[], false).to_string();
    let (field, members) = struct_layout(&wgsl, "Field");
    assert_eq!(field, 192);
    assert_eq!(
        members.iter().find(|(n, _)| n == "source_count").unwrap().1,
        128
    );
    assert_eq!(
        members.iter().find(|(n, _)| n == "closed_input").unwrap().1,
        176
    );
}

/// Every entry module is registered in [`SHADER_MODULES`].
#[test]
fn entry_modules_are_registered() {
    for name in ENTRY_MODULES {
        let path = format!("package::{name}");
        assert!(SHADER_MODULES.iter().any(|(p, _)| *p == path), "{path}");
    }
}

#[test]
fn scan_and_sort_sizes_match_rust() {
    let main = "import package::scan::SCAN_BLOCK;\n\
                import package::sort::{SORT_BLOCK, SORT_DIGITS};\n\
                @group(1) @binding(0) var<storage, read_write> out: array<u32>;\n\
                @compute @workgroup_size(1) fn main() {\n\
                out[0] = SCAN_BLOCK; out[1] = SORT_BLOCK; out[2] = SORT_DIGITS; }";
    let mut interp = Interpreter::new(compile_main(main));
    interp.bind(1, 0, &[0; 12]);
    interp.run("main");
    assert_eq!(
        words(&interp.read(1, 0)),
        [SCAN_BLOCK, SORT_BLOCK, SORT_DIGITS]
    );
}

#[test]
fn constants_match_rust() {
    let main = "import package::field::{COS_MAX_PUSH_TILT, SIN_MAX_PUSH_TILT, \
                LUMP_RADIUS_MIN, LUMP_RADIUS_MAX, LUMP_HEIGHT_MIN, MAX_DIRECTIONS};\n\
                import package::collapse::{MIN_CORNER_COS, MIN_QUALITY};\n\
                import package::geometry::TWO_SQRT_3;\n\
                import package::math::{F32_MAX, F32_MIN_POSITIVE};\n\
                @group(0) @binding(0) var<storage, read_write> out: array<u32>;\n\
                @compute @workgroup_size(1) fn main() {\n\
                out[0] = bitcast<u32>(COS_MAX_PUSH_TILT);\n\
                out[1] = bitcast<u32>(SIN_MAX_PUSH_TILT);\n\
                out[2] = bitcast<u32>(LUMP_RADIUS_MIN);\n\
                out[3] = bitcast<u32>(LUMP_RADIUS_MAX);\n\
                out[4] = bitcast<u32>(LUMP_HEIGHT_MIN);\n\
                out[5] = MAX_DIRECTIONS;\n\
                out[6] = bitcast<u32>(MIN_CORNER_COS);\n\
                out[7] = bitcast<u32>(MIN_QUALITY);\n\
                out[8] = bitcast<u32>(TWO_SQRT_3);\n\
                out[9] = bitcast<u32>(F32_MAX);\n\
                out[10] = bitcast<u32>(F32_MIN_POSITIVE);\n}";
    let mut interp = Interpreter::new(compile_main(main));
    interp.bind(0, 0, &[0; 44]);
    interp.run("main");
    let out = words(&interp.read(0, 0));
    let tilt = std::f32::consts::FRAC_PI_3;
    let expected = [
        tilt.cos().to_bits(),
        tilt.sin().to_bits(),
        LUMP_RADIUS_MIN.to_bits(),
        LUMP_RADIUS_MAX.to_bits(),
        LUMP_HEIGHT_MIN.to_bits(),
        MAX_DIRECTIONS as u32,
        MIN_CORNER_COS.to_bits(),
        MIN_QUALITY.to_bits(),
        (2.0 * 3f32.sqrt()).to_bits(),
        f32::MAX.to_bits(),
        f32::MIN_POSITIVE.to_bits(),
    ];
    assert_eq!(out, expected);
}

/// Fields for every preset plus a few direction setups, built on a ground patch.
fn test_fields(ground: &SurfaceMesh) -> Vec<(String, ClumpField<'_>)> {
    let mut options: Vec<(String, ClumpOptions)> = ClumpPreset::all()
        .map(|p| (p.key.to_string(), p.clump.clone()))
        .collect();
    let mut sideways = ClumpOptions {
        directions: vec![
            Direction::DOWN,
            Direction::new([1.0, -0.3, 0.2]),
            Direction {
                strength: 0.4,
                ..Direction::new([0.0, 0.0, -1.0])
            },
        ],
        default_seed: 0xdead_beef,
        ..ClumpOptions::default()
    };
    sideways.shape.min_support = 0.2;
    options.push(("sideways".into(), sideways));
    let mut everywhere = ClumpOptions::default();
    everywhere.mask.any_facing = true;
    everywhere.mask.exposure_power = 2.5;
    everywhere.shape.sink = 0.0;
    options.push(("any facing".into(), everywhere));
    options
        .into_iter()
        .map(|(name, o)| (name, ClumpField::new(ground.surface().unwrap(), &o)))
        .collect()
}

/// Packing settings straight from options, as GPU requests do, gives the
/// same words as packing a prepared CPU field, apart from the two values
/// measured from the input.
#[test]
fn options_pack_like_a_prepared_field() {
    let ground = SurfaceMesh::ground(1.0, 2);
    let mut all: Vec<ClumpOptions> = ClumpPreset::all().map(|p| p.clump.clone()).collect();
    all.push(ClumpOptions {
        directions: vec![Direction::DOWN, Direction::new([1.0, -0.3, 0.2])],
        scale: 2.5,
        ..ClumpOptions::default()
    });
    for options in all {
        let field = ClumpField::new(ground.surface().unwrap(), &options);
        let mut prepared = field.gpu_words();
        // `epsilon` and `closed_input` come from the input.
        prepared[43] = 0;
        prepared[44] = 0;
        assert_eq!(crate::field::field_words(&options), prepared);
    }
}

/// Returns a random surface point; some have infinite or zero room, and
/// some sit below `min_support`.
fn random_point(rng: &mut Rng) -> SurfacePoint {
    let mut p = SurfacePoint::new(random_vec(rng) * 3.0, random_unit(rng));
    for e in &mut p.exposure {
        *e = rng.range(-0.1, 1.1);
    }
    p.room = match (rng.next_f32() * 4.0) as u32 {
        0 => f32::INFINITY,
        1 => 0.0,
        _ => rng.range(0.0, 0.5),
    };
    p.rise = rng.range(0.0, 0.05);
    p.dip = rng.range(0.0, 0.05);
    p
}

/// Pushes `p` in the input layout `read_point` expects: point, normal,
/// exposure, room, rise, dip (17 words).
fn push_point(words: &mut Vec<u32>, p: &SurfacePoint) {
    push3(words, p.point);
    push3(words, p.normal);
    words.extend(p.exposure.map(f32::to_bits));
    words.extend([p.room, p.rise, p.dip].map(f32::to_bits));
}

/// WESL that reads a `Point` written by [`push_point`] at word `i`.
const READ_POINT: &str = "
fn read_point(i: u32) -> Point {
    var e: array<f32, 8>;
    for (var d = 0u; d < 8u; d++) { e[d] = f(i + 6u + d); }
    return Point(v3(i), v3(i + 3u), e, f(i + 14u), f(i + 15u), f(i + 16u));
}";

#[test]
fn coverage_and_heights_match_cpu() {
    let ground = SurfaceMesh::ground(1.0, 2);
    let mut rng = Rng::new(11);
    let points: Vec<SurfacePoint> = (0..40).map(|_| random_point(&mut rng)).collect();
    let weights: Vec<f32> = (0..40).map(|_| rng.range(-0.2, 1.2)).collect();
    let mut input = Vec::new();
    for (p, w) in points.iter().zip(&weights) {
        push_point(&mut input, p);
        input.push(w.to_bits());
    }

    // Per point: facing, mask, weight, top, fade, lump height, layer top.
    let body = format!(
        "for (var n = 0u; n < {count}u; n++) {{\n\
           let p = read_point(n * 18u);\n\
           let w = f(n * 18u + 17u);\n\
           put(n * 7u, facing(field, p.normal));\n\
           put(n * 7u + 1u, mask(field, p));\n\
           put(n * 7u + 2u, weight(field, p));\n\
           put(n * 7u + 3u, top(field, w));\n\
           put(n * 7u + 4u, fade(field, top(field, w)));\n\
           put(n * 7u + 5u, lump_height(field, p.point));\n\
           put(n * 7u + 6u, layer_top(field, p, w));\n\
         }}",
        count = points.len()
    );
    let imports = format!(
        "import package::field::{{Point, facing, mask, weight, top, fade, lump_height, layer_top}};\n{READ_POINT}"
    );
    for (name, field) in test_fields(&ground) {
        let out = run_kernel(
            &imports,
            &field.gpu_words(),
            &input,
            &body,
            7 * points.len(),
        );
        for (n, (p, &w)) in points.iter().zip(&weights).enumerate() {
            let got = |k: usize| f32::from_bits(out[7 * n + k]);
            let at = |what: &str| format!("{name}, point {n}: {what}");
            assert_bits(got(0), field.facing(p.normal), &at("facing"));
            assert_bits(got(1), field.mask(p), &at("mask"));
            assert_bits(got(2), field.weight(p), &at("weight"));
            assert_bits(got(3), field.top(w), &at("top"));
            assert_bits(got(4), field.fade(field.top(w)), &at("fade"));
            assert_bits(got(5), field.lump_height(p.point), &at("lump height"));
            assert_bits(got(6), layer_top(&field, p, w), &at("layer top"));
        }
    }
}

#[test]
fn tilt_and_lowering_match_cpu() {
    let mut rng = Rng::new(5);
    let mut cases = Vec::new();
    for n in 0..60 {
        let normal = random_unit(&mut rng);
        // Some directions straight against the normal, where the rotation
        // plane is undefined.
        let dir = if n % 10 == 0 {
            -normal
        } else {
            random_unit(&mut rng)
        };
        let (top, floor) = (rng.range(-0.1, 0.2), rng.range(0.0, 0.02));
        // Some tops within the snapping distance of the floor.
        let top = if n % 7 == 0 { floor + 1.5e-6 } else { top };
        cases.push((dir, normal, top, floor));
    }
    let mut input = Vec::new();
    for &(dir, normal, top, floor) in &cases {
        push3(&mut input, dir);
        push3(&mut input, normal);
        input.extend([top.to_bits(), floor.to_bits()]);
    }
    let body = format!(
        "for (var n = 0u; n < {count}u; n++) {{\n\
           put3(n * 4u, limit_tilt(v3(n * 8u), v3(n * 8u + 3u)));\n\
           put(n * 4u + 3u, lowered(f(n * 8u + 6u), f(n * 8u + 7u)));\n\
         }}",
        count = cases.len()
    );
    let field = [0; 48];
    let out = run_kernel(
        "import package::field::{limit_tilt, lowered};",
        &field,
        &input,
        &body,
        4 * cases.len(),
    );
    for (n, &(dir, normal, top, floor)) in cases.iter().enumerate() {
        let tilted = limit_tilt(dir, normal).to_array();
        for k in 0..3 {
            assert_bits(
                f32::from_bits(out[4 * n + k]),
                tilted[k],
                &format!("tilt {n}"),
            );
        }
        assert_bits(
            f32::from_bits(out[4 * n + 3]),
            lowered(top, floor),
            &format!("lowered {n}"),
        );
    }
}

#[test]
fn collapse_checks_match_cpu() {
    let mut rng = Rng::new(9);
    let edge_length = 0.4;
    let mut cases = Vec::new();
    for n in 0..80 {
        // A small triangle near the origin, its corner normals, and a merge.
        let before = [0; 3].map(|_| random_vec(&mut rng) * 0.3);
        let face = (before[1] - before[0])
            .cross(before[2] - before[0])
            .normalize_or(Vec3::Y);
        let normals = [0; 3].map(|_| (face + random_vec(&mut rng) * 0.8).normalize_or(face));
        let moved = [n % 3 == 0, n % 3 == 1, n % 3 != 1 && n % 2 == 0];
        let placement = Placement {
            point: before[n % 3] + random_vec(&mut rng) * 0.2,
            normal: (face + random_vec(&mut rng) * 0.5).normalize_or(face),
        };
        let outer = [random_vec(&mut rng) * 0.3, random_vec(&mut rng) * 0.3];
        cases.push((before, normals, moved, placement, outer));
    }
    let mut input = Vec::new();
    for (before, normals, moved, placement, outer) in &cases {
        for v in before.iter().chain(normals) {
            push3(&mut input, *v);
        }
        input.extend(moved.map(u32::from));
        push3(&mut input, placement.point);
        push3(&mut input, placement.normal);
        push3(&mut input, outer[0]);
        push3(&mut input, outer[1]);
    }
    let body = format!(
        "for (var n = 0u; n < {count}u; n++) {{\n\
           let i = n * 33u;\n\
           let before = array<vec3<f32>, 3>(v3(i), v3(i + 3u), v3(i + 6u));\n\
           let normals = array<vec3<f32>, 3>(v3(i + 9u), v3(i + 12u), v3(i + 15u));\n\
           let moved = vec3<bool>(input[i + 18u] != 0u, input[i + 19u] != 0u, input[i + 20u] != 0u);\n\
           put(n * 2u, collapse_quality(before, normals, moved, v3(i + 21u), v3(i + 24u), {edge}));\n\
           let near = border_stays_near(v3(i + 21u), v3(i + 27u), v3(i + 30u), before[0], before[1], {edge});\n\
           out[n * 2u + 1u] = select(0u, 1u, near);\n\
         }}",
        count = cases.len(),
        edge = edge_length
    );
    let out = run_kernel(
        "import package::collapse::{collapse_quality, border_stays_near};",
        &[0; 48],
        &input,
        &body,
        2 * cases.len(),
    );
    let mut refused = 0;
    for (n, (before, normals, moved, placement, outer)) in cases.iter().enumerate() {
        let cpu = collapse_quality(*before, *normals, *moved, *placement, edge_length);
        let gpu = f32::from_bits(out[2 * n]);
        match cpu {
            Some(q) => assert_bits(gpu, q, &format!("collapse {n}")),
            None => {
                assert_eq!(gpu, -1.0, "collapse {n} should be refused");
                refused += 1;
            }
        }
        let near = border_stays_near(placement.point, *outer, [before[0], before[1]], edge_length);
        assert_eq!(out[2 * n + 1] != 0, near, "border {n}");
    }
    // Both outcomes are exercised.
    assert!(
        refused > 5 && refused < cases.len() - 5,
        "{refused} refused"
    );
}

#[test]
fn triangle_crossing_matches_cpu() {
    let mut rng = Rng::new(21);
    let mut cases = Vec::new();
    for n in 0..80 {
        let a = [0; 3].map(|_| random_vec(&mut rng));
        let mut b = [0; 3].map(|_| random_vec(&mut rng));
        // Some pairs share a corner or an edge, which only touch.
        if n % 5 == 0 {
            b[0] = a[1];
        }
        if n % 9 == 0 {
            b[1] = a[2];
        }
        cases.push((a, b));
    }
    let mut input = Vec::new();
    for (a, b) in &cases {
        for v in a.iter().chain(b) {
            push3(&mut input, *v);
        }
    }
    let body = format!(
        "for (var n = 0u; n < {count}u; n++) {{\n\
           let i = n * 18u;\n\
           let a = array<vec3<f32>, 3>(v3(i), v3(i + 3u), v3(i + 6u));\n\
           let b = array<vec3<f32>, 3>(v3(i + 9u), v3(i + 12u), v3(i + 15u));\n\
           out[n] = select(0u, 1u, triangles_cross(a, b));\n\
         }}",
        count = cases.len()
    );
    let out = run_kernel(
        "import package::geometry::triangles_cross;",
        &[0; 48],
        &input,
        &body,
        cases.len(),
    );
    let mut crossing = 0;
    for (n, (a, b)) in cases.iter().enumerate() {
        let cpu = triangles_cross(*a, *b);
        crossing += cpu as usize;
        assert_eq!(out[n] != 0, cpu, "pair {n}");
    }
    assert!(
        crossing > 5 && crossing < cases.len() - 5,
        "{crossing} crossing"
    );
}

#[test]
fn hash_matches_cpu() {
    let inputs = [
        [0, 0, 0],
        [1, 2, 3],
        [u32::MAX, 7, 0x8000_0000],
        [12345, 678, 9],
    ];
    for v in inputs {
        let body = format!(
            "let h = package::hash::pcg3d(vec3<u32>({}u, {}u, {}u));\n\
             out[0] = h.x; out[1] = h.y; out[2] = h.z;\n\
             out[3] = bitcast<u32>(package::hash::unit_float(h.x));",
            v[0], v[1], v[2]
        );
        let out = run_main("", &body, 4);
        let h = pcg3d(v);
        assert_eq!(out[..3], h, "{v:?}");
        assert_eq!(f32::from_bits(out[3]), unit_float(h[0]), "{v:?}");
    }
}

#[test]
fn rng_draws_match_cpu() {
    for seed in [0, 3, 0x0b10_b5ed] {
        let mut rng = Rng::new(seed);
        for counter in 0..6 {
            let body =
                format!("out[0] = bitcast<u32>(package::hash::rng_draw({seed}u, {counter}u));");
            let out = run_main("", &body, 1);
            assert_eq!(f32::from_bits(out[0]), rng.next_f32(), "{seed} #{counter}");
        }
    }
}

/// The interpreter runs the atomic operations the generator's hash tables,
/// counters and merge selection rely on, so kernels using them can be
/// tested without a GPU.
#[test]
fn interpreter_runs_atomics() {
    let main = r#"
@group(0) @binding(0) var<storage, read_write> table: array<atomic<u32>>;
@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    atomicAdd(&table[0], 1u);
    atomicMin(&table[1], 100u - i);
    atomicMax(&table[2], i * 3u);
    // Only the first invocation to arrive claims slot 3.
    let claim = atomicCompareExchangeWeak(&table[3], 0xffffffffu, i);
    if claim.exchanged {
        atomicAdd(&table[4], 1u);
    }
    atomicOr(&table[5], 1u << (i % 32u));
}
"#;
    let mut interp = Interpreter::new(compile_main(main));
    interp.bind(0, 0, &bytes_of([0, u32::MAX, 0, u32::MAX, 0, 0]));
    interp.dispatch("main", 40);
    let table = words(&interp.read(0, 0));
    assert_eq!(table, [40, 61, 117, 0, 1, u32::MAX]);
}
