//! Putting a canonical mesh in its one canonical order, and hashing it.

use procgena_core::ContentHasher;

use crate::carve::FeatureKind;
use crate::feature::FeatureId;

use super::mesh::{TinChunk, TinConstraint, TinTriangle, TinVertex};
use super::numeric::MmHeight;
use super::probe::ProbeAddr;

impl TinChunk {
    /// The same mesh in canonical order: vertices sorted by z, then x;
    /// each triangle starting at its smallest index (keeping its winding),
    /// then sorted; each constraint from its smaller index, then sorted;
    /// and unmet tolerances sorted. Meshes that differ only in order are
    /// equal once normalized.
    pub fn normalized(&self) -> Self {
        // Vertices in order, and where each one went.
        let mut order: Vec<u32> = (0..self.vertices.len() as u32).collect();
        order.sort_by_key(|&i| {
            let v = self.vertices[i as usize];
            (v.z, v.x, v.h)
        });
        let mut moved_to = vec![0u32; order.len()];
        for (new, &old) in order.iter().enumerate() {
            moved_to[old as usize] = new as u32;
        }
        let vertices: Vec<TinVertex> = order.iter().map(|&i| self.vertices[i as usize]).collect();

        // Triangles, each rotated to start at its smallest index.
        let mut triangles: Vec<TinTriangle> = self
            .triangles
            .iter()
            .map(|t| {
                let v = t.0.map(|i| moved_to[i as usize]);
                let first = (0..3).min_by_key(|&k| v[k]).expect("three corners");
                TinTriangle([v[first], v[(first + 1) % 3], v[(first + 2) % 3]])
            })
            .collect();
        triangles.sort();

        // Constraints, each from its smaller index.
        let mut constraints: Vec<TinConstraint> = self
            .constraints
            .iter()
            .map(|c| {
                let (a, b) = (moved_to[c.v[0] as usize], moved_to[c.v[1] as usize]);
                TinConstraint {
                    v: [a.min(b), a.max(b)],
                    ..*c
                }
            })
            .collect();
        constraints.sort();
        let mut unmet = self.unmet.clone();
        unmet.sort();
        let mut dropped = self.dropped.clone();
        dropped.sort();
        Self {
            key: self.key,
            profile: self.profile,
            vertices,
            triangles,
            constraints,
            unmet,
            dropped,
        }
    }

    /// A hash of the mesh's content: equal meshes hash the same on every
    /// platform, whatever order their vertices, triangles and constraints
    /// are in.
    pub fn content_hash(&self) -> u64 {
        let mesh = self.normalized();
        let mut h = ContentHasher::new();
        h.write_str("procgena_terrain::TinChunk");
        h.write_u32(mesh.profile.code());
        h.write_i64(mesh.key.x);
        h.write_i64(mesh.key.z);
        h.write_u32(u32::from(mesh.key.level));
        h.write_usize(mesh.vertices.len());
        for v in &mesh.vertices {
            h.write_i64(v.x.0);
            h.write_i64(v.z.0);
            h.write_i32(v.h.0);
        }
        h.write_usize(mesh.triangles.len());
        for t in &mesh.triangles {
            for i in t.0 {
                h.write_u32(i);
            }
        }
        h.write_usize(mesh.constraints.len());
        for c in &mesh.constraints {
            h.write_u32(c.v[0]);
            h.write_u32(c.v[1]);
            h.write_u32(c.kind.code());
            write_feature(&mut h, c.feature);
        }
        h.write_usize(mesh.unmet.len());
        for u in &mesh.unmet {
            h.write_i64(u.at.0.0);
            h.write_i64(u.at.1.0);
            h.write_i32(u.error.0);
        }
        // Written only when there are some, so meshes without features
        // keep the hashes they had before pieces could be dropped.
        if !mesh.dropped.is_empty() {
            h.write_str("dropped");
            h.write_usize(mesh.dropped.len());
            for d in &mesh.dropped {
                for (x, z) in [d.from, d.to] {
                    h.write_i64(x.0);
                    h.write_i64(z.0);
                }
                h.write_u32(d.kind.code());
                write_feature(&mut h, Some(d.feature));
            }
        }
        h.finish()
    }
}

/// Writes an optional feature id to `h`.
fn write_feature(h: &mut ContentHasher, feature: Option<FeatureId>) {
    match feature {
        None => h.write_u32(0),
        Some(id) => {
            h.write_u32(match id.kind {
                FeatureKind::River => 1,
                FeatureKind::Road => 2,
                FeatureKind::Pond => 3,
            });
            h.write_u64(id.key);
        }
    }
}

/// A hash of heights sampled at probes: the first stage of building a
/// canonical mesh. Equal on every platform for equal inputs.
pub fn probes_hash(probes: &[ProbeAddr], heights: &[MmHeight]) -> u64 {
    let mut h = ContentHasher::new();
    h.write_str("procgena_terrain::probes");
    h.write_usize(probes.len());
    for (p, height) in probes.iter().zip(heights) {
        h.write_i64(p.x.0);
        h.write_i64(p.z.0);
        h.write_i32(height.0);
    }
    h.finish()
}
