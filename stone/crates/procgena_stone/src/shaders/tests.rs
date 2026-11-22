//! Checks that the WESL shaders compile, pass naga validation, agree with the
//! Rust constants and buffer layouts, and reproduce the CPU generator when run
//! in the WESL interpreter.

use std::collections::HashMap;

use glam::{Mat3, Vec2, Vec3};
use naga::valid::{Capabilities, ModuleInfo, ValidationFlags, Validator};
use naga::{AddressSpace as NagaSpace, TypeInner};
use wesl::eval::{Context, EvalAttrs, Exec, Inputs, Instance, RefInstance, Type, ty_eval_ty};
use wesl::resolver::VirtualResolver;
use wesl::syntax::{AccessMode, AddressSpace, GlobalDeclaration};
use wesl::{CompileOptions, CompileResult, Compiler};

use super::*;
use crate::field::{HALF_EXTENT, SURFACE_OFFSET};
use crate::grid::SampleGrid;
use crate::hash::{PCG_INCREMENT, PCG_MULTIPLIER, pcg3d, pcg3d_unit};
use crate::mesh::dc::REGULARIZATION;
use crate::mesh::tables::{MC_CORNERS, MC_EDGES, TRI_TABLE};
use crate::mesh::{crossing_edges, crossing_position};
use crate::noise::{FBM_OFFSET, FBM_ROTATION, Roughness, TRIPLANAR_MIN_WEIGHT, fbm, value_noise};
use crate::options::{MAX_CARVE_STEPS, PlacementOptions};
use crate::placement::{
    DIRECTION_COUNT, GOLDEN_FRACTION, MAX_DESCENT_STEPS, NEARLY_WIDEST_PERCENT, NEIGHBOUR_COS,
    PROFILE_KERNEL, REFINE_CANDIDATES, REFINE_ROUNDS, REFINE_SPREAD, StonePlacement,
};
use crate::{Mesher, StoneField, StoneMesh, StoneOptions, mesh_field};

/// Modules with compute entry points.
const ENTRY_MODULES: [&str; 5] = [
    "package::generate",
    "package::mc",
    "package::dc",
    "package::scan",
    "package::place",
];

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

fn compile(root: &str) -> CompileResult {
    compile_with(root, &[], false)
}

/// Compiles a test module that can import every shader module.
fn compile_main(main: &str) -> CompileResult {
    compile_with("package::main", &[("package::main", main)], true)
}

fn naga_module(compiled: &CompileResult) -> (naga::Module, ModuleInfo) {
    let wgsl = compiled.to_string();
    let module = naga::front::wgsl::parse_str(&wgsl)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&wgsl)));
    let info = Validator::new(ValidationFlags::all(), Capabilities::default())
        .validate(&module)
        .unwrap_or_else(|e| panic!("naga validation failed: {e:?}"));
    (module, info)
}

#[test]
fn every_module_passes_naga_validation() {
    for (path, _) in SHADER_MODULES {
        naga_module(&compile(path));
    }
    for path in ENTRY_MODULES {
        naga_module(&compile_with(path, &[], true));
    }
}

#[test]
fn entry_points_fit_webgpu_binding_limits() {
    for path in ENTRY_MODULES {
        let (module, info) = naga_module(&compile_with(path, &[], true));
        for (i, entry) in module.entry_points.iter().enumerate() {
            let uses = info.get_entry_point(i);
            let count = |space: fn(&NagaSpace) -> bool| {
                module
                    .global_variables
                    .iter()
                    .filter(|(h, var)| space(&var.space) && !uses[*h].is_empty())
                    .count()
            };
            let storage = count(|s| matches!(s, NagaSpace::Storage { .. }));
            let uniform = count(|s| matches!(s, NagaSpace::Uniform));
            assert!(
                storage <= 8,
                "{path}::{}: {storage} storage buffers",
                entry.name
            );
            assert!(
                uniform <= 12,
                "{path}::{}: {uniform} uniform buffers",
                entry.name
            );
        }
    }
}

/// Returns the span and `(name, offset)` members of the struct containing `member`.
fn struct_layout(module: &naga::Module, member: &str) -> (u32, Vec<(String, u32)>) {
    module
        .types
        .iter()
        .find_map(|(_, ty)| match &ty.inner {
            TypeInner::Struct { members, span }
                if members.iter().any(|m| m.name.as_deref() == Some(member)) =>
            {
                let members = members
                    .iter()
                    .map(|m| (m.name.clone().unwrap(), m.offset))
                    .collect();
                Some((*span, members))
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("no struct with member {member}"))
}

#[test]
fn buffer_layouts_match_rust_packing() {
    let (module, _) = naga_module(&compile_with("package::dc", &[], true));

    // The order and offsets written by `pack_request`.
    let (span, members) = struct_layout(&module, "spheres");
    assert_eq!(span as usize, REQUEST_SIZE);
    let expected = [
        "carve_scale",
        "base_radius",
        "smoothness",
        "displacement",
        "spread",
        "seed",
        "octaves",
        "steps",
        "resolution",
        "request_index",
        "vertex_base",
        "vertex_capacity",
        "index_base",
        "index_capacity",
        "crossing_capacity",
        "placement_index",
        "settle",
        "max_embed_fraction",
        "min_embed_fraction",
    ];
    let mut offsets: Vec<(String, u32)> = expected
        .iter()
        .enumerate()
        .map(|(i, name)| {
            (
                name.to_string(),
                if i == 0 { 0 } else { 4 * (i as u32 + 2) },
            )
        })
        .collect();
    offsets.push(("spheres".into(), 96));
    assert_eq!(members, offsets);

    assert_eq!(struct_layout(&module, "position").0 as u64, VERTEX_SIZE);
    assert_eq!(
        struct_layout(&module, "base_vertex").0 as u64,
        DRAW_ARGS_SIZE
    );
    // Check status field offsets, including matrix padding, against the Rust decoder.
    let (span, members) = struct_layout(&module, "crossing_count");
    assert_eq!(span as u64, STATUS_SIZE);
    let offsets: Vec<(&str, u32)> = members.iter().map(|(n, o)| (n.as_str(), *o)).collect();
    assert_eq!(
        offsets,
        [
            ("overflow", 0),
            ("vertex_count", 4),
            ("index_count", 8),
            ("crossing_count", 12),
            ("rotation", 16),
            ("translation", 64),
            ("embed_depth", 76),
        ]
    );
}

/// Evaluates a constant expression in `root` and returns its memory as words.
fn eval_words(compiled: &CompileResult, expr: &str) -> Vec<u32> {
    let mut result = compiled
        .eval(expr)
        .unwrap_or_else(|e| panic!("{expr}: {e}"));
    let bytes = result.to_buffer().unwrap();
    bytes
        .chunks(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect()
}

fn eval_f32(compiled: &CompileResult, expr: &str) -> f32 {
    f32::from_bits(eval_words(compiled, expr)[0])
}

#[test]
fn constants_match_rust() {
    let hash = compile("package::hash");
    assert_eq!(eval_words(&hash, "PCG_MULTIPLIER"), [PCG_MULTIPLIER]);
    assert_eq!(eval_words(&hash, "PCG_INCREMENT"), [PCG_INCREMENT]);

    let noise = compile("package::noise");
    for (row, name) in ["FBM_ROTATION_ROW0", "FBM_ROTATION_ROW1"]
        .iter()
        .enumerate()
    {
        for (col, &expected) in FBM_ROTATION[row].iter().enumerate() {
            let expr = format!("{name}[{col}]");
            assert_eq!(eval_f32(&noise, &expr), expected, "{expr}");
        }
    }
    assert_eq!(eval_f32(&noise, "FBM_OFFSET"), FBM_OFFSET);
    assert_eq!(
        eval_f32(&noise, "TRIPLANAR_MIN_WEIGHT"),
        TRIPLANAR_MIN_WEIGHT
    );

    let generate = compile("package::generate");
    assert_eq!(eval_words(&generate, "WORKGROUP_SIZE"), [WORKGROUP_SIZE]);
    assert_eq!(
        eval_words(&generate, "OVERFLOW_VERTICES"),
        [OVERFLOW_VERTICES]
    );
    assert_eq!(
        eval_words(&generate, "OVERFLOW_INDICES"),
        [OVERFLOW_INDICES]
    );
    assert_eq!(
        eval_words(&generate, "OVERFLOW_CROSSINGS"),
        [OVERFLOW_CROSSINGS]
    );
    let scan = compile("package::scan");
    assert_eq!(eval_words(&scan, "SCAN_BLOCK"), [SCAN_BLOCK]);

    let field = compile("package::field");
    assert_eq!(eval_f32(&field, "SURFACE_OFFSET"), SURFACE_OFFSET);
    assert_eq!(eval_f32(&field, "HALF_EXTENT"), HALF_EXTENT);
    assert_eq!(eval_words(&field, "MAX_CARVE_STEPS"), [MAX_CARVE_STEPS]);

    let tables = compile("package::tables");
    for (i, corner) in MC_CORNERS.iter().enumerate() {
        assert_eq!(
            eval_words(&tables, &format!("MC_CORNERS[{i}]"))[..3],
            corner[..]
        );
    }
    for (i, (offset, axis)) in MC_EDGES.iter().enumerate() {
        assert_eq!(
            eval_words(&tables, &format!("MC_EDGE_OFFSETS[{i}]"))[..3],
            offset[..]
        );
        assert_eq!(
            eval_words(&tables, &format!("MC_EDGE_AXES[{i}]")),
            [*axis as u32]
        );
    }

    let dc = compile("package::dc");
    assert_eq!(eval_f32(&dc, "REGULARIZATION"), REGULARIZATION);

    let place = compile("package::place");
    assert_eq!(eval_words(&place, "DIRECTION_COUNT"), [DIRECTION_COUNT]);
    assert_eq!(eval_words(&place, "MAX_DESCENT_STEPS"), [MAX_DESCENT_STEPS]);
    assert_eq!(eval_f32(&place, "NEIGHBOUR_COS"), NEIGHBOUR_COS);
    assert_eq!(eval_words(&place, "REFINE_CANDIDATES"), [REFINE_CANDIDATES]);
    assert_eq!(eval_f32(&place, "REFINE_SPREAD"), REFINE_SPREAD);
    assert_eq!(eval_f32(&place, "GOLDEN_FRACTION"), GOLDEN_FRACTION);
    assert_eq!(eval_f32(&place, "TAU"), std::f32::consts::TAU);
    assert_eq!(eval_words(&place, "MAX_PROFILE_BINS"), [MAX_PROFILE_BINS]);
    assert_eq!(eval_words(&place, "STATE_WORDS"), [PLACE_STATE_WORDS]);
    assert_eq!(eval_words(&place, "PROFILE_KERNEL"), PROFILE_KERNEL);
    assert_eq!(
        eval_words(&place, "NEARLY_WIDEST_PERCENT"),
        [NEARLY_WIDEST_PERCENT]
    );
    assert_eq!(
        eval_words(&place, "PROFILE_KERNEL_TOTAL"),
        [PROFILE_KERNEL.iter().sum::<u32>()]
    );
}

/// Runs GPU placement code without a GPU, using CPU-generated surface inputs.
/// Returns one status entry as floats for the supplied field, grid, and settings.
fn run_placement(field: &StoneField, resolution: u32, placement: &PlacementOptions) -> Vec<f32> {
    // Supply field samples, surface points, and their count as earlier GPU passes would.
    let grid = SampleGrid::sample(field, resolution);
    let n = (resolution + 1).pow(3);
    let samples: Vec<u32> = (0..n)
        .map(|i| {
            let s = resolution + 1;
            grid.value([i % s, i / s % s, i / (s * s)]).to_bits()
        })
        .collect();
    let edges = crossing_edges(&grid);
    let crossings: Vec<u32> = edges
        .iter()
        .flat_map(|&(s, axis)| {
            let p = crossing_position(&grid, s, axis);
            [p.x, p.y, p.z, 0.0, 0.0, 0.0].map(f32::to_bits)
        })
        .collect();
    let mut prefix = vec![0; 2 * (n as usize + 1)];
    prefix[2 * n as usize] = edges.len() as u32;
    let slots = RequestSlots {
        request_index: 1,
        crossing_capacity: edges.len() as u32,
        placement_index: 1,
        ..RequestSlots::default()
    };

    // Execute the same placement pass sequence used for GPU generation.
    let mut interp = Interpreter::new(compile_with("package::place", &[], true));
    interp.bind(0, 0, &pack_request(field, resolution, &slots, placement));
    interp.bind(0, 2, &bytes_of(samples));
    interp.bind(0, 3, &bytes_of(prefix));
    interp.bind(0, 4, &bytes_of(crossings));
    interp.bind(0, 8, &bytes_of(vec![0; 2 * STATUS_WORDS]));
    interp.bind(
        0,
        9,
        &bytes_of(vec![0; place_scratch_words(resolution) as usize]),
    );
    interp.dispatch("place_com_slices", resolution + 1);
    interp.run("place_com");
    interp.dispatch("place_heights", DIRECTION_COUNT);
    interp.run("place_descend");
    for _ in 0..REFINE_ROUNDS {
        interp.dispatch("place_refine_heights", REFINE_CANDIDATES);
        interp.run("place_refine_pick");
    }
    interp.run("place_finish");
    interp.dispatch("place_profile_slices", resolution + 1);
    interp.run("place_embed");
    floats(&interp.read(0, 8))[STATUS_WORDS..].to_vec()
}

/// Checks a complete shader status entry against CPU rotation, offset, and burial depth.
fn assert_placement_matches(gpu: &[f32], cpu: &StonePlacement) {
    let rotation = cpu.rotation();
    for col in 0..3 {
        for row in 0..3 {
            let (g, c) = (gpu[4 + 4 * col + row], rotation.col(col)[row]);
            assert!((g - c).abs() < 1e-5, "rotation[{col}][{row}]: {g} vs {c}");
        }
    }
    let translation = cpu.translation();
    for axis in 0..3 {
        let (g, c) = (gpu[16 + axis], translation[axis]);
        assert!((g - c).abs() < 1e-5, "translation[{axis}]: {g} vs {c}");
    }
    assert_eq!(gpu[19], cpu.embed_depth(), "embed depth");
}

/// Checks CPU/shader agreement with settling off, with and without a burial cap.
#[test]
fn placement_kernels_reproduce_cpu_without_settling() {
    let resolution = 12;
    let mut blocky = StoneOptions::default();
    blocky.carve.blockiness = 1.0;
    for options in [StoneOptions::default(), blocky] {
        let field = StoneField::new(&options).unwrap();
        let grid = SampleGrid::sample(&field, resolution);
        for (max_embed_fraction, min_embed_fraction) in [(1.0, 0.05), (0.3, 0.0), (0.5, 0.5)] {
            let placement = PlacementOptions {
                settle: false,
                max_embed_fraction,
                min_embed_fraction,
            };
            let gpu = run_placement(&field, resolution, &placement);
            assert_placement_matches(&gpu, &StonePlacement::from_grid(&grid, &placement));
        }
    }
}

/// Checks CPU/shader agreement when settling a regular rock and a thin slab. Run with
/// `cargo test -p procgena_stone --lib -- --ignored slow`.
#[test]
#[ignore = "slow: interprets about 700 analysis invocations, some with long loops (minutes)"]
fn slow_placement_kernels_reproduce_cpu_settling() {
    let resolution = 12;
    for stretch in [[1.0, 1.0, 1.0], [1.0, 0.8, 0.3]] {
        let options = StoneOptions {
            stretch,
            ..StoneOptions::default()
        };
        let field = StoneField::new(&options).unwrap();
        let grid = SampleGrid::sample(&field, resolution);
        let gpu = run_placement(&field, resolution, &options.placement);
        assert_placement_matches(&gpu, &StonePlacement::from_grid(&grid, &options.placement));
    }
}

#[test]
fn packed_tri_table_round_trips() {
    for (case, (entry, row)) in packed_tri_table().iter().zip(TRI_TABLE).enumerate() {
        let edges: Vec<i8> = row.iter().copied().take_while(|&e| e >= 0).collect();
        assert_eq!(entry[2] as usize, edges.len(), "case {case}");
        assert!(edges.len() < 16, "case {case}");
        for (k, &edge) in edges.iter().enumerate() {
            let word = entry[k / 8];
            assert_eq!(
                (word >> (4 * (k % 8))) & 15,
                edge as u32,
                "case {case} edge {k}"
            );
        }
    }
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

    /// Returns the declared type and address space of the variable at `group`, `binding`.
    fn binding_type(&self, group: u32, binding: u32) -> (Type, AddressSpace) {
        let mut ctx = Context::new(&self.compiled.syntax);
        self.compiled.syntax.exec(&mut ctx).unwrap();
        for decl in &self.compiled.syntax.global_declarations {
            if let GlobalDeclaration::Declaration(d) = decl.node()
                && d.attr_group_binding(&mut ctx).ok() == Some((group, binding))
            {
                let ty = ty_eval_ty(d.ty.as_ref().unwrap(), &mut ctx).unwrap();
                let space = match d.kind {
                    wesl::syntax::DeclarationKind::Var(Some((space, _))) => space,
                    _ => panic!("binding {group}:{binding} is not a buffer"),
                };
                return (ty, space);
            }
        }
        panic!("no binding {group}:{binding}");
    }

    /// Binds `bytes` as the variable at `group`, `binding`.
    fn bind(&mut self, group: u32, binding: u32, bytes: &[u8]) {
        let (ty, space) = self.binding_type(group, binding);
        let inst = Instance::from_buffer(bytes, &ty).unwrap_or_else(|| {
            panic!(
                "binding {group}:{binding}: {} bytes do not fit {ty:?}",
                bytes.len()
            )
        });
        let access = if space == AddressSpace::Uniform {
            AccessMode::Read
        } else {
            AccessMode::ReadWrite
        };
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

fn floats(bytes: &[u8]) -> Vec<f32> {
    words(bytes).into_iter().map(f32::from_bits).collect()
}

fn bytes_of(words: impl IntoIterator<Item = u32>) -> Vec<u8> {
    words.into_iter().flat_map(u32::to_le_bytes).collect()
}

fn wgsl_f32(x: f32) -> String {
    format!("bitcast<f32>({}u)", x.to_bits())
}

fn wgsl_vec3(v: Vec3) -> String {
    format!(
        "vec3<f32>({}, {}, {})",
        wgsl_f32(v.x),
        wgsl_f32(v.y),
        wgsl_f32(v.z)
    )
}

/// Runs `body` in a compute entry point whose `out` array receives the results,
/// and returns `out` as floats.
fn run_main(imports: &str, body: &str, outputs: usize) -> Vec<f32> {
    let main = format!(
        r#"
{imports}
@group(1) @binding(0) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(1)
fn main() {{
{body}
}}
"#
    );
    let mut interp = Interpreter::new(compile_main(&main));
    interp.bind(1, 0, &vec![0; 16 * outputs]);
    interp.run("main");
    floats(&interp.read(1, 0))
}

#[test]
fn hash_matches_cpu() {
    let inputs = [
        [0, 0, 0],
        [1, 2, 3],
        [u32::MAX, 880, 0xdead_beef],
        [7, u32::MAX, 12],
    ];
    let body: String = inputs
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let v = format!("vec3<u32>({}u, {}u, {}u)", v[0], v[1], v[2]);
            format!(
                "out[{}] = bitcast<vec4<f32>>(vec4<u32>(pcg3d({v}), 0u));\n\
                 out[{}] = vec4<f32>(pcg3d_unit({v}), 0.0);\n",
                2 * i,
                2 * i + 1
            )
        })
        .collect();
    let out = run_main(
        "import package::hash::{pcg3d, pcg3d_unit};",
        &body,
        2 * inputs.len(),
    );
    for (i, v) in inputs.iter().enumerate() {
        let hash: Vec<u32> = out[8 * i..8 * i + 3].iter().map(|f| f.to_bits()).collect();
        assert_eq!(hash, pcg3d(*v), "pcg3d({v:?})");
        assert_eq!(
            out[8 * i + 4..8 * i + 7],
            pcg3d_unit(*v),
            "pcg3d_unit({v:?})"
        );
    }
}

#[test]
fn noise_matches_cpu() {
    let points = [
        Vec2::new(0.25, -1.75),
        Vec2::new(-3.0, 5.0),
        Vec2::new(123.456, -98.7),
        Vec2::new(-0.001, 0.999),
    ];
    let mut body = String::new();
    for (i, p) in points.iter().enumerate() {
        let v = format!("vec2<f32>({}, {})", wgsl_f32(p.x), wgsl_f32(p.y));
        body += &format!("out[{}] = vec4<f32>(value_noise({v}, 9u), 0.0);\n", 2 * i);
        body += &format!(
            "out[{}] = vec4<f32>(fbm({v}, {}u, 6u), 0.0);\n",
            2 * i + 1,
            i
        );
    }
    let roughness = Roughness {
        seed: 4,
        spread: 10.0,
        octaves: 6,
    };
    let p = Vec3::new(0.11, -0.23, 0.37);
    let normals = [Vec3::Z, Vec3::ZERO, Vec3::new(0.3, -2.0, 0.5)];
    for (i, n) in normals.iter().enumerate() {
        body += &format!(
            "out[{}] = vec4<f32>(triplanar({}, {}, 4u, 10.0, 6u), 0.0);\n",
            2 * points.len() + i,
            wgsl_vec3(p),
            wgsl_vec3(*n)
        );
    }
    let out = run_main(
        "import package::noise::{fbm, triplanar, value_noise};",
        &body,
        2 * points.len() + normals.len(),
    );
    let at = |k: usize| Vec3::new(out[4 * k], out[4 * k + 1], out[4 * k + 2]);
    for (i, p) in points.iter().enumerate() {
        assert_eq!(at(2 * i), value_noise(*p, 9), "value_noise({p})");
        assert_eq!(at(2 * i + 1), fbm(*p, i as u32, 6), "fbm({p})");
    }
    for (i, n) in normals.iter().enumerate() {
        assert_eq!(
            at(2 * points.len() + i),
            roughness.triplanar(p, *n),
            "triplanar({n})"
        );
    }
}

#[test]
fn normalize_or_matches_glam() {
    let cases = [
        Vec3::ZERO,
        Vec3::new(3.0, -4.0, 12.0),
        Vec3::new(1e-20, 0.0, 0.0),
        Vec3::new(1e-39, 0.0, 0.0),
        Vec3::new(f32::MAX, f32::MAX, 0.0),
    ];
    let fallback = Vec3::new(0.0, 0.0, -1.0);
    let body: String = cases
        .iter()
        .enumerate()
        .map(|(i, v)| {
            format!(
                "out[{i}] = vec4<f32>(normalize_or({}, {}), 0.0);\n",
                wgsl_vec3(*v),
                wgsl_vec3(fallback)
            )
        })
        .collect();
    let out = run_main("import package::field::normalize_or;", &body, cases.len());
    for (i, v) in cases.iter().enumerate() {
        let shader = Vec3::new(out[4 * i], out[4 * i + 1], out[4 * i + 2]);
        assert_eq!(shader, v.try_normalize().unwrap_or(fallback), "{v}");
    }
}

#[test]
fn dual_contouring_solve_matches_glam() {
    let normals = [
        Vec3::new(0.0, 0.0, 1.0),
        Vec3::new(0.6, 0.8, 0.0),
        Vec3::new(-0.48, 0.6, 0.64),
    ];
    let rhs = Vec3::new(0.3, -0.7, 0.2);
    let mut a = Mat3::from_diagonal(Vec3::splat(REGULARIZATION));
    for n in normals {
        a += Mat3::from_cols(n * n.x, n * n.y, n * n.z);
    }
    let body = format!(
        "out[0] = vec4<f32>(solve3(mat3x3<f32>({}, {}, {}), {}), 0.0);",
        wgsl_vec3(a.x_axis),
        wgsl_vec3(a.y_axis),
        wgsl_vec3(a.z_axis),
        wgsl_vec3(rhs)
    );
    let out = run_main("import package::dc::solve3;", &body, 1);
    assert_eq!(Vec3::new(out[0], out[1], out[2]), a.inverse() * rhs);
}

/// Settings covering default, stretched, and smooth rocks.
fn test_options() -> Vec<StoneOptions> {
    let mut stretched = StoneOptions {
        seed: 3,
        stretch: [1.0, 0.6, 0.8],
        ..StoneOptions::default()
    };
    stretched.carve.steps = 30;
    let mut smooth = StoneOptions {
        seed: 7,
        ..StoneOptions::default()
    };
    smooth.surface.displacement_scale = 0.0;
    vec![StoneOptions::default(), stretched, smooth]
}

#[test]
fn field_matches_cpu() {
    let points = [
        Vec3::ZERO,
        Vec3::new(0.3, 0.1, -0.2),
        Vec3::new(-0.25, 0.28, 0.05),
        Vec3::new(0.12, -0.33, 0.31),
        Vec3::new(0.5, 0.2, -0.1),
        Vec3::new(-0.41, -0.07, 0.36),
    ];
    let step = 0.01;
    let body: String = points
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let p = wgsl_vec3(*p);
            format!(
                "out[{i}] = vec4<f32>(stone_gradient({p}, {}), stone_distance({p}));\n",
                wgsl_f32(step)
            )
        })
        .collect();
    let main = format!(
        r#"
import package::generate::{{stone_distance, stone_gradient}};
@group(1) @binding(0) var<storage, read_write> out: array<vec4<f32>>;
@compute @workgroup_size(1)
fn main() {{
{body}
}}
"#
    );
    for options in test_options() {
        let field = StoneField::new(&options).unwrap();
        let mut interp = Interpreter::new(compile_main(&main));
        let request = pack_request(
            &field,
            8,
            &RequestSlots::default(),
            &PlacementOptions::default(),
        );
        interp.bind(0, 0, &request);
        interp.bind(1, 0, &vec![0; 16 * points.len()]);
        interp.run("main");
        let out = floats(&interp.read(1, 0));
        for (i, p) in points.iter().enumerate() {
            let gradient = Vec3::new(out[4 * i], out[4 * i + 1], out[4 * i + 2]);
            assert_eq!(
                out[4 * i + 3],
                field.distance(*p),
                "seed {} at {p}",
                options.seed
            );
            assert_eq!(
                gradient,
                field.gradient(*p, step),
                "seed {} at {p}",
                options.seed
            );
        }
    }
}

/// Output slots used by the kernel tests: nonzero bases check that results land
/// in the right part of shared buffers.
const SLOTS: RequestSlots = RequestSlots {
    request_index: 1,
    vertex_base: 3,
    vertex_capacity: 4000,
    index_base: 5,
    index_capacity: 20000,
    crossing_capacity: 4000,
    placement_index: 0,
};

/// Words per `Vertex`, `DrawArgs`, and `Status`.
const VERTEX_WORDS: usize = 6;
const DRAW_WORDS: usize = 5;
const STATUS_WORDS: usize = 20;

/// Creates `count` status entries with no rotation, offset, or burial depth.
/// This leaves generated meshes in their original stone-space coordinates.
fn identity_status(count: usize) -> Vec<u32> {
    let mut words = vec![0; count * STATUS_WORDS];
    for entry in words.chunks_mut(STATUS_WORDS) {
        for diagonal in [4, 9, 14] {
            entry[diagonal] = 1f32.to_bits();
        }
    }
    words
}

/// Runs every generator pass except the prefix sum in the interpreter, doing
/// the prefix sum on the CPU, and returns the output buffers.
struct KernelOutput {
    samples: Vec<f32>,
    vertices: Vec<f32>,
    indices: Vec<u32>,
    draw_args: Vec<u32>,
    status: Vec<u32>,
}

fn run_kernels(field: &StoneField, mesher: Mesher, resolution: u32) -> KernelOutput {
    let request = pack_request(field, resolution, &SLOTS, &PlacementOptions::default());
    let n = (resolution + 1).pow(3);

    let mut sampler = Interpreter::new(compile_with("package::generate", &[], true));
    sampler.bind(0, 0, &request);
    sampler.bind(0, 2, &bytes_of(vec![0; n as usize]));
    sampler.dispatch("sample_field", n);
    let samples = sampler.read(0, 2);

    let (root, count, emits, finish): (_, _, &[&str], _) = match mesher {
        Mesher::MarchingCubes => (
            "package::mc",
            "count_mc",
            &["emit_mc_vertices", "emit_mc_indices"],
            "finish_mc",
        ),
        Mesher::DualContouring => (
            "package::dc",
            "count_dc",
            &["emit_dc_crossings", "emit_dc_vertices", "emit_dc_indices"],
            "finish_dc",
        ),
    };
    let mut mesher = Interpreter::new(compile_with(root, &[], true));
    mesher.bind(0, 0, &request);
    if root == "package::mc" {
        mesher.bind(0, 1, &bytes_of(packed_tri_table().into_iter().flatten()));
    } else {
        let len = SLOTS.crossing_capacity as usize * VERTEX_WORDS;
        mesher.bind(0, 4, &bytes_of(vec![0; len]));
    }
    mesher.bind(0, 2, &samples);
    mesher.bind(0, 3, &bytes_of(vec![0; 2 * (n as usize + 1)]));
    let vertex_len = (SLOTS.vertex_base + SLOTS.vertex_capacity) as usize * VERTEX_WORDS;
    mesher.bind(0, 5, &bytes_of(vec![0; vertex_len]));
    let index_len = (SLOTS.index_base + SLOTS.index_capacity) as usize;
    mesher.bind(0, 6, &bytes_of(vec![0; index_len]));
    mesher.bind(0, 7, &bytes_of(vec![0; 2 * DRAW_WORDS]));
    mesher.bind(0, 8, &bytes_of(identity_status(2)));

    // Count, then replace the counts with their exclusive prefix sum.
    mesher.dispatch(count, n + 1);
    let counts = words(&mesher.read(0, 3));
    let mut total = [0u32; 2];
    let mut prefix = Vec::with_capacity(counts.len());
    for pair in counts.chunks(2) {
        prefix.extend(total);
        total = [total[0] + pair[0], total[1] + pair[1]];
    }
    mesher.bind(0, 3, &bytes_of(prefix));

    for emit in emits {
        mesher.dispatch(emit, n);
    }
    mesher.run(finish);

    KernelOutput {
        samples: floats(&samples),
        vertices: floats(&mesher.read(0, 5)),
        indices: words(&mesher.read(0, 6)),
        draw_args: words(&mesher.read(0, 7)),
        status: words(&mesher.read(0, 8)),
    }
}

/// Checks the kernel output against the CPU mesh, exactly.
fn assert_matches_cpu(out: &KernelOutput, cpu: &StoneMesh, mesher: Mesher) {
    let vertex_count = cpu.positions.len();
    let index_count = cpu.indices.len();
    let request = SLOTS.request_index as usize;
    let crossings = if mesher == Mesher::DualContouring {
        index_count as u32 / 6
    } else {
        0
    };
    assert_eq!(
        out.status[request * STATUS_WORDS..][..4],
        [0, vertex_count as u32, index_count as u32, crossings]
    );
    // Mesh output must retain the placement supplied in its source status entry.
    assert_eq!(
        out.status[request * STATUS_WORDS..][4..STATUS_WORDS],
        identity_status(1)[4..]
    );
    assert_eq!(
        out.draw_args[request * DRAW_WORDS..][..DRAW_WORDS],
        [
            index_count as u32,
            1,
            SLOTS.index_base,
            SLOTS.vertex_base,
            0
        ]
    );

    let base = SLOTS.vertex_base as usize;
    for (v, (position, normal)) in cpu.positions.iter().zip(&cpu.normals).enumerate() {
        let gpu = &out.vertices[(base + v) * VERTEX_WORDS..][..VERTEX_WORDS];
        assert_eq!(gpu[..3], position[..], "{mesher:?} vertex {v} position");
        assert_eq!(gpu[3..], normal[..], "{mesher:?} vertex {v} normal");
    }
    let first = SLOTS.index_base as usize;
    assert_eq!(
        out.indices[first..first + index_count],
        cpu.indices[..],
        "{mesher:?} indices"
    );
}

/// Runs every pass except the prefix sum through the interpreter and compares
/// the result with the CPU mesher. Run with `cargo test -p procgena_stone --lib -- --ignored slow`.
#[test]
#[ignore = "slow: interprets about 3,000 kernel invocations per mesher (minutes)"]
fn slow_kernels_reproduce_cpu_meshes() {
    let resolution = 8;
    let mut options = StoneOptions::default();
    options.surface.octaves = 2;
    let field = StoneField::new(&options).unwrap();
    let grid = SampleGrid::sample(&field, resolution);
    let expected_samples: Vec<f32> = (0..(resolution + 1).pow(3))
        .map(|i| {
            let n = resolution + 1;
            grid.value([i % n, i / n % n, i / (n * n)])
        })
        .collect();

    for mesher in [Mesher::MarchingCubes, Mesher::DualContouring] {
        let out = run_kernels(&field, mesher, resolution);
        assert_eq!(out.samples, expected_samples);
        let cpu = mesh_field(&field, mesher, resolution).unwrap();
        assert!(cpu.triangle_count() > 0);
        assert_matches_cpu(&out, &cpu, mesher);
    }
}

/// The WGSL compiled by `build.rs` passes validation and keeps every entry point.
#[cfg(feature = "gpu")]
#[test]
fn build_script_wgsl_is_valid() {
    for (name, wgsl) in COMPILED_WGSL {
        let module = naga::front::wgsl::parse_str(wgsl)
            .unwrap_or_else(|e| panic!("{name}: {}", e.emit_to_string(wgsl)));
        Validator::new(ValidationFlags::all(), Capabilities::default())
            .validate(&module)
            .unwrap_or_else(|e| panic!("{name}: naga validation failed: {e:?}"));
        let (expected, _) = naga_module(&compile_with(&format!("package::{name}"), &[], true));
        let entries = |m: &naga::Module| {
            m.entry_points
                .iter()
                .map(|e| e.name.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(entries(&module), entries(&expected), "{name}");
    }
}
