//! With the `gpu` feature, compiles the generator's WESL shaders to WGSL in `OUT_DIR`.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "gpu")]
    compile_shaders();
}

/// Shader modules with entry points, compiled to `<module>.wgsl`. Keep in
/// step with `ENTRY_MODULES` in `src/shaders.rs`.
#[cfg(feature = "gpu")]
const ENTRY_MODULES: [&str; 14] = [
    "scan", "sort", "ingest", "weld", "bvh", "measure", "scatter", "output", "patch", "simplify",
    "envelope", "refine", "shell", "close",
];

/// Writes `<module>.wgsl` to `OUT_DIR` for each module in [`ENTRY_MODULES`].
#[cfg(feature = "gpu")]
fn compile_shaders() {
    use std::path::PathBuf;

    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let shaders = manifest.join("shaders");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo::rerun-if-changed={}", shaders.display());

    let compiler = wesl::Compiler::default();
    for module in ENTRY_MODULES {
        let path = format!("package::{module}").parse().unwrap();
        let wgsl = compiler
            .compile_module(&shaders, &path)
            .unwrap_or_else(|e| panic!("compiling shaders/{module}.wesl: {e}"))
            .to_string();
        std::fs::write(out.join(format!("{module}.wgsl")), wgsl).unwrap();
    }
}
