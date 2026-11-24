//! Saves every preset as OBJ and GLB files, each with all four default
//! detail levels, in `target/procgena_mushroom_export/`.
//!
//! ```bash
//! cargo run -p procgena_mushroom --example export --features export-obj,export-gltf
//! ```

use std::fs;
use std::path::Path;

use procgena_mushroom::export::{gltf, obj};
use procgena_mushroom::{Mushroom, MushroomPreset, default_lod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/procgena_mushroom_export");
    fs::create_dir_all(&dir)?;

    for preset in MushroomPreset::ALL {
        let mushroom = Mushroom::from_preset(preset);
        let lod = default_lod();
        let meshes: Vec<_> = mushroom
            .lod_meshes(&lod)?
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        let name = preset.name().to_lowercase().replace(' ', "_");

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
