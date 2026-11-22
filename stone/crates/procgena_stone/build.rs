//! With the `gpu` feature, compiles the generator's WESL shaders to WGSL in `OUT_DIR`.

fn main() {
    println!("cargo::rerun-if-changed=build.rs");
    #[cfg(feature = "gpu")]
    compile_shaders();
}

/// Writes `<module>.wgsl` to `OUT_DIR` for each shader module with entry points.
#[cfg(feature = "gpu")]
fn compile_shaders() {
    use std::path::PathBuf;

    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let shaders = manifest.join("shaders");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!("cargo::rerun-if-changed={}", shaders.display());

    let compiler = wesl::Compiler::default();
    for module in ["generate", "mc", "dc", "scan", "place"] {
        let path = format!("package::{module}").parse().unwrap();
        let wgsl = compiler
            .compile_module(&shaders, &path)
            .unwrap_or_else(|e| panic!("compiling shaders/{module}.wesl: {e}"))
            .to_string();
        std::fs::write(out.join(format!("{module}.wgsl")), wgsl).unwrap();
    }
}
