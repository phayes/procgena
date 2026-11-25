//! Saves every preset as OBJ and GLB files, each with its game-level mesh
//! and its lower mesh, in `target/procgena_mushroom_export/`.
//!
//! ```bash
//! cargo run -p procgena_mushroom --example export --features export-obj,export-gltf
//! ```

use std::fs;
use std::path::Path;

use procgena_mushroom::export::{gltf, obj};
use procgena_mushroom::{Mushroom, MushroomPreset};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/procgena_mushroom_export");
    fs::create_dir_all(&dir)?;

    for preset in MushroomPreset::all() {
        let mushroom = Mushroom::from_preset(preset)?;
        let meshes = mushroom.lod_meshes().map(|(_, mesh)| mesh);
        let name = &preset.key;

        let files = obj::to_obj(&meshes, mushroom.options(), &format!("{name}.mtl"));
        fs::write(dir.join(format!("{name}.obj")), files.obj)?;
        fs::write(dir.join(format!("{name}.mtl")), files.mtl)?;
        fs::write(
            dir.join(format!("{name}.glb")),
            gltf::to_glb(&meshes, mushroom.options())?,
        )?;
        println!("{name}: {} levels", meshes.len());
    }
    println!("wrote {}", dir.display());
    Ok(())
}
