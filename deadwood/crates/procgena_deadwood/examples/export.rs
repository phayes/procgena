//! Saves every preset as OBJ and GLB files, each holding all of its default
//! levels of detail, plus a kindling scatter as one GLB file. The files go
//! in `target/procgena_deadwood_export/` at the top of the workspace.
//!
//! ```bash
//! cargo run -p procgena_deadwood --example export --features export-obj,export-gltf
//! ```

use std::fs;
use std::path::Path;

use glam::{DVec2, Vec2};
use procgena_core::scatter::{ScatterArea, ScatterInput};
use procgena_deadwood::export::{gltf, obj};
use procgena_deadwood::{DeadwoodPreset, Piece, ScatterOptions, StickScatter, default_lod};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target/procgena_deadwood_export");
    fs::create_dir_all(&dir)?;

    // Every preset, one object per level of detail.
    for preset in DeadwoodPreset::all() {
        let piece = Piece::from_preset(preset)?;
        let meshes: Vec<_> = piece
            .lod_meshes(&default_lod())?
            .into_iter()
            .map(|(_, m)| m)
            .collect();
        let name = &preset.key;

        let files = obj::to_obj(&meshes, piece.options(), &format!("{name}.mtl"));
        fs::write(dir.join(format!("{name}.obj")), files.obj)?;
        fs::write(dir.join(format!("{name}.mtl")), files.mtl)?;
        fs::write(
            dir.join(format!("{name}.glb")),
            gltf::to_glb(&meshes, piece.options())?,
        )?;
        let triangles: Vec<_> = meshes.iter().map(|m| m.triangle_count()).collect();
        println!(
            "{name}: {} stems, triangles per level {triangles:?}, burial {:.3} m",
            piece.stem_count(),
            piece.pose().burial
        );
    }

    // A kindling scatter on gently rolling ground. Each placed stick is
    // copied and moved into place, and saved as its own object in one file.
    // Strips are dropped, since a file can't keep them facing the camera.
    let ground = |p: DVec2| {
        let (x, z) = (p.x as f32, p.y as f32);
        0.15 * (x * 0.8).sin() + 0.1 * (z * 1.1).cos()
    };
    let kindling = ScatterOptions::kindling();
    let area = ScatterArea::new(DVec2::ZERO, Vec2::splat(6.0));
    let input = ScatterInput::new(&ground, area, kindling.default_seed);
    let scatter = StickScatter::new(&kindling, &input)?;
    let mut meshes = Vec::new();
    for instance in scatter.instances() {
        let mut mesh = scatter.variants()[instance.variant as usize].mesh();
        for p in &mut mesh.surfaces.positions {
            *p = instance.transform.transform_point3((*p).into()).to_array();
        }
        for n in &mut mesh.surfaces.normals {
            *n = instance
                .transform
                .transform_vector3((*n).into())
                .normalize()
                .to_array();
        }
        // Turn tangents too, keeping their bitangent sign (rotations don't
        // mirror).
        for t in &mut mesh.surfaces.tangents {
            let turned = instance
                .transform
                .transform_vector3(glam::Vec3::new(t[0], t[1], t[2]))
                .normalize();
            *t = [turned.x, turned.y, turned.z, t[3]];
        }
        mesh.ribbons = None;
        meshes.push(mesh);
    }
    let options = scatter.variants()[0].options();
    fs::write(dir.join("kindling.glb"), gltf::to_glb(&meshes, options)?)?;
    println!("kindling: {} sticks", scatter.instances().len());

    println!("wrote {}", dir.display());
    Ok(())
}
