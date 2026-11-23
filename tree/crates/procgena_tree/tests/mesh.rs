//! Check mesh layout, detail levels, texture and lighting data, and closed bark.

use glam::{Quat, Vec3};
use procgena_tree::{
    BarkUvMode, Billboard, BillboardMode, BranchShape, LodDetail, MeshBuffers, Section, Skeleton,
    TreeMesh, TreeOptions, TreePreset, default_lod_levels,
};

/// Checks the default switching distances and branch and leaf settings.
/// Fruit and flower defaults are covered in `tests/cluster_mesh.rs`.
#[test]
fn default_lod_levels_have_the_documented_settings() {
    let levels = default_lod_levels();
    assert_eq!(levels.map(|l| l.distance), [0.0, 10.0, 25.0, 50.0]);
    assert_eq!(levels.map(|l| l.hysteresis), [0.0, 0.05, 0.05, 0.05]);
    assert_eq!(levels[0].detail, LodDetail::default());

    let [_, near, far, farthest] = levels.map(|l| l.detail);
    let bark = |d: LodDetail| (d.section_stride, d.segment_factor, d.branch_shape);
    assert_eq!(bark(near), (3, 0.75, BranchShape::Tube));
    assert_eq!(bark(far), (6, 0.4, BranchShape::Tube));
    assert_eq!(bark(farthest), (8, 1.0, BranchShape::Ribbons));
    // Only the gameplay levels, LOD0 and LOD1, close their bark.
    assert_eq!(levels.map(|l| l.detail.closed), [true, true, false, false]);
    assert_eq!(
        levels.map(|l| l.detail.min_branch_radius),
        [0.0, 0.0, 0.0, 0.015]
    );

    let leaves = |d: LodDetail| (d.leaf_stride, d.leaf_scale, d.billboard);
    assert_eq!(leaves(near), (2, 1.25, None));
    assert_eq!(leaves(far), (2, 1.3, Some(Billboard::Single)));
    assert_eq!(leaves(farthest), (4, 1.8, Some(Billboard::Single)));
    // Camera-facing leaf rotation is enabled only for LOD3.
    for detail in [near, far] {
        assert_eq!(detail.leaf_billboard, None);
    }
    assert_eq!(farthest.leaf_billboard, Some(BillboardMode::Camera));
    assert_eq!(farthest.leaf_billboard_scale, 1.25);
}

/// Checks that spans cover every buffer entry exactly once and identify the source geometry.
#[test]
fn spans_cover_the_buffers() {
    let options = TreeOptions::default();
    let skeleton = Skeleton::generate(&options).unwrap();
    let levels = default_lod_levels().into_iter().flat_map(|level| {
        [true, false].map(|closed| procgena_tree::LodLevel {
            detail: LodDetail {
                closed,
                ..level.detail
            },
            ..level
        })
    });
    for level in levels {
        let mesh = skeleton.mesh(&options, &level.detail);
        let stride = level.detail.leaf_stride as usize;
        let capped = level.detail.closed && level.detail.branch_shape == BranchShape::Tube;

        // Branch spans must cover the vertex and index buffers in order, without gaps or overlap.
        assert_eq!(mesh.branch_spans.len(), skeleton.branches.len());
        let (mut vertex, mut index) = (0, 0);
        for (i, span) in mesh.branch_spans.iter().enumerate() {
            assert_eq!(span.branch, i);
            assert_eq!(span.level, skeleton.branches[i].level);
            assert_eq!((span.vertices.start, span.indices.start), (vertex, index));
            assert_eq!(span.shape, level.detail.branch_shape);

            // Omitted thin branches keep empty spans; the trunk is never omitted.
            let branch = &skeleton.branches[i];
            if branch.level > 0 && branch.base_radius < level.detail.min_branch_radius {
                assert!(span.sections.is_empty());
                assert!(span.vertices.is_empty() && span.indices.is_empty());
                continue;
            }
            let rings = span.sections.len() as u32;
            assert_eq!(span.vertices.len() as u32, rings * span.ring_vertices());
            // Closed tubes may add caps and fan point tips; their closure is
            // checked by `closed_bark_is_watertight_and_faces_outward`.
            if !capped {
                assert_eq!(span.indices.len() as u32, (rings - 1) * span.segments * 6);
            }
            assert_eq!(span.sections.first(), Some(&0));
            assert_eq!(
                span.sections.last(),
                Some(&(skeleton.branches[i].sections.len() - 1))
            );
            (vertex, index) = (span.vertices.end, span.indices.end);
        }
        assert_eq!(vertex as usize, mesh.branches.vertex_count());
        assert_eq!(index as usize, mesh.branches.indices.len());

        // Only retained leaves get spans; together they must cover both leaf buffers completely.
        let kept = kept_leaves(&skeleton, stride);
        let leaves: Vec<usize> = mesh.leaf_spans.iter().map(|s| s.leaf).collect();
        assert_eq!(leaves, kept);
        let (mut vertex, mut index) = (0, 0);
        for span in &mesh.leaf_spans {
            assert_eq!((span.vertices.start, span.indices.start), (vertex, index));
            (vertex, index) = (span.vertices.end, span.indices.end);
        }
        assert_eq!(vertex as usize, mesh.leaves.vertex_count());
        assert_eq!(index as usize, mesh.leaves.indices.len());
    }
}

/// Returns expected leaf indices in skeleton order. On each branch, select
/// every `stride`th leaf starting nearest its tip.
fn kept_leaves(skeleton: &Skeleton, stride: usize) -> Vec<usize> {
    let stride = stride.max(1);
    let mut by_branch: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for (i, leaf) in skeleton.leaves.iter().enumerate() {
        by_branch.entry(leaf.parent.branch).or_default().push(i);
    }
    let along = |i: usize| {
        let parent = &skeleton.leaves[i].parent;
        parent.section as f32 + parent.t
    };
    let mut kept: Vec<usize> = by_branch
        .into_values()
        .flat_map(|mut group| {
            group.sort_by(|&a, &b| along(b).total_cmp(&along(a)).then(a.cmp(&b)));
            group.into_iter().step_by(stride).collect::<Vec<_>>()
        })
        .collect();
    kept.sort_unstable();
    kept
}

/// Checks that tip-first selection keeps each branch’s outermost leaf and
/// rounds the retained count up when `n` leaves are divided by the stride.
#[test]
fn leaf_stride_keeps_each_branch_tip() {
    for preset in [
        TreePreset::AspenSmall,
        TreePreset::OakMedium,
        TreePreset::PineMedium,
    ] {
        let options = preset.options();
        let skeleton = Skeleton::generate(&options).unwrap();
        for stride in [2, 3, 4, 7] {
            let detail = LodDetail {
                leaf_stride: stride,
                ..LodDetail::default()
            };
            let mesh = skeleton.mesh(&options, &detail);
            let kept: Vec<usize> = mesh.leaf_spans.iter().map(|s| s.leaf).collect();
            assert_eq!(
                kept,
                kept_leaves(&skeleton, stride as usize),
                "{preset:?} {stride}"
            );

            let mut counts: std::collections::BTreeMap<usize, (usize, usize)> = Default::default();
            for (i, leaf) in skeleton.leaves.iter().enumerate() {
                counts.entry(leaf.parent.branch).or_default().0 += 1;
                if kept.binary_search(&i).is_ok() {
                    counts.entry(leaf.parent.branch).or_default().1 += 1;
                }
            }
            for (branch, (all, kept_here)) in counts {
                let case = format!("{preset:?} stride {stride} branch {branch}");
                assert_eq!(kept_here, all.div_ceil(stride as usize), "{case}");
                let outermost = (0..skeleton.leaves.len())
                    .filter(|&i| skeleton.leaves[i].parent.branch == branch)
                    .max_by(|&a, &b| {
                        let along = |i: usize| {
                            let p = &skeleton.leaves[i].parent;
                            p.section as f32 + p.t
                        };
                        along(a).total_cmp(&along(b)).then(b.cmp(&a))
                    })
                    .unwrap();
                assert!(kept.binary_search(&outermost).is_ok(), "{case}");
            }
        }
    }
}

/// Checks that enlarging cards preserves their original centres, and that
/// a size multiplier of 1 preserves the original corners.
#[test]
fn scaled_leaves_stay_centred() {
    let options = TreePreset::OakSmall.options();
    let skeleton = Skeleton::generate(&options).unwrap();
    let mesh_at = |leaf_scale| {
        let detail = LodDetail {
            leaf_scale,
            billboard: Some(Billboard::Single),
            ..LodDetail::default()
        };
        skeleton.mesh(&options, &detail).leaves
    };
    let (unscaled, scaled) = (mesh_at(1.0), mesh_at(1.8));
    for (i, leaf) in skeleton.leaves.iter().enumerate() {
        let corners = |buffers: &MeshBuffers| -> Vec<Vec3> {
            (4 * i..4 * i + 4)
                .map(|v| Vec3::from_array(buffers.positions[v]))
                .collect()
        };
        let (a, b) = (corners(&unscaled), corners(&scaled));
        let centre = leaf.origin + leaf.orientation * Vec3::Y * (leaf.size / 2.0);
        let mean = |c: &[Vec3]| c.iter().sum::<Vec3>() / 4.0;
        let tolerance = 1e-4 * leaf.size.max(1.0);
        assert!(mean(&a).abs_diff_eq(centre, tolerance), "leaf {i}");
        assert!(mean(&b).abs_diff_eq(centre, tolerance), "leaf {i}");
        let side = |c: &[Vec3]| c[1].distance(c[2]);
        assert!((side(&a) - leaf.size).abs() < tolerance, "leaf {i}");
        assert!((side(&b) - leaf.size * 1.8).abs() < tolerance, "leaf {i}");
    }
}

#[test]
fn every_index_is_in_range() {
    let options = TreeOptions::default();
    let mesh = Skeleton::generate(&options)
        .unwrap()
        .mesh(&options, &LodDetail::default());
    for buffers in [&mesh.branches, &mesh.leaves] {
        let count = buffers.vertex_count() as u32;
        assert!(buffers.indices.iter().all(|&i| i < count));
        assert_eq!(buffers.indices.len() % 3, 0);
    }
}

/// Returns default tree options with the requested bark texture layout,
/// followed by a skeleton generated from those options.
fn default_tree(uv_mode: BarkUvMode) -> (TreeOptions, Skeleton) {
    let mut options = TreeOptions::default();
    options.bark.uv_mode = uv_mode;
    let skeleton = Skeleton::generate(&options).unwrap();
    (options, skeleton)
}

/// Checks that changing the bark texture layout only changes coordinates
/// along branches, leaving geometry and leaves unchanged at every detail level.
#[test]
fn continuous_uvs_change_only_v() {
    let (mirrored_options, skeleton) = default_tree(BarkUvMode::Mirrored);
    let (continuous_options, _) = default_tree(BarkUvMode::Continuous);
    for level in default_lod_levels() {
        let mirrored = skeleton.mesh(&mirrored_options, &level.detail);
        let continuous = skeleton.mesh(&continuous_options, &level.detail);
        assert_eq!(continuous.leaves, mirrored.leaves);
        assert_eq!(continuous.branch_spans, mirrored.branch_spans);
        let (a, b) = (&continuous.branches, &mirrored.branches);
        assert_eq!(
            (&a.positions, &a.normals, &a.tangents, &a.indices),
            (&b.positions, &b.normals, &b.tangents, &b.indices)
        );
        let u = |m: &MeshBuffers| m.uvs.iter().map(|uv| uv[0]).collect::<Vec<_>>();
        assert_eq!(u(a), u(b));

        // All vertices in a ring keep the original section number as v,
        // including when lower detail skips intermediate sections.
        for span in &continuous.branch_spans {
            let ring = span.ring_vertices() as usize;
            for (k, &section) in span.sections.iter().enumerate() {
                let first = span.vertices.start as usize + k * ring;
                for uv in &a.uvs[first..first + ring] {
                    assert_eq!(uv[1], section as f32);
                }
            }
        }
    }
}

/// Checks that rounded leaf normals stay unit length on double billboards at
/// leaf size 2, where one corner's offset nearly cancels the face normal.
#[test]
fn rounded_leaf_normals_are_unit_at_size_two() {
    let mut options = TreeOptions::default();
    options.leaves.billboard = Billboard::Double;
    options.leaves.rounded_normals = true;
    let mut skeleton = Skeleton::generate(&options).unwrap();
    for leaf in &mut skeleton.leaves {
        leaf.size = 2.0;
    }
    let mesh = skeleton.mesh(&options, &LodDetail::default());
    assert!(!mesh.leaves.normals.is_empty());
    for n in &mesh.leaves.normals {
        let n = Vec3::from_array(*n);
        assert!(n.is_finite() && (n.length() - 1.0).abs() < 1e-5, "{n:?}");
    }
}

/// Checks the lighting directions used by bark and leaf normal maps: tangents
/// are unit length and perpendicular to normals, with handedness +1 or -1.
/// Camera-facing buffers must omit both normals and tangents.
#[test]
fn bark_and_leaf_tangents_are_unit_and_perpendicular() {
    for preset in TreePreset::ALL {
        let mut options = preset.options();
        options.bark.uv_mode = BarkUvMode::Continuous;
        let skeleton = Skeleton::generate(&options).unwrap();
        for level in default_lod_levels() {
            let mesh = skeleton.mesh(&options, &level.detail);
            for buffers in [&mesh.branches, &mesh.leaves] {
                if !buffers.billboard.is_empty() {
                    // Billboard bark has no normals; billboard leaves keep one per card.
                    assert_eq!(buffers.billboard.len(), buffers.vertex_count());
                    assert!(buffers.tangents.is_empty());
                    let leaves = std::ptr::eq(buffers, &mesh.leaves);
                    let expected = if leaves { buffers.vertex_count() } else { 0 };
                    assert_eq!(buffers.normals.len(), expected);
                    continue;
                }
                assert_eq!(buffers.tangents.len(), buffers.vertex_count());
                for (t, n) in buffers.tangents.iter().zip(&buffers.normals) {
                    let xyz = Vec3::new(t[0], t[1], t[2]);
                    assert!((xyz.length() - 1.0).abs() < 1e-5, "{t:?}");
                    assert!(xyz.dot(Vec3::from_array(*n)).abs() < 1e-5, "{t:?} {n:?}");
                    assert!(t[3] == 1.0 || t[3] == -1.0, "{t:?}");
                }
            }
        }
    }
}

/// Checks that bark tangents point in the same general directions as the
/// texture coordinates on a gently bending, twisting branch.
/// This fixture does not cover sharp bends or twists, where triangle texture
/// directions can differ from the smooth-surface directions of the tangents.
#[test]
fn bark_tangents_follow_the_texture_coordinates() {
    // Advance one unit along the branch, then bend by 0.15 radians and twist
    // by 0.1 radians for the next section.
    let mut sections = Vec::new();
    let (mut origin, mut orientation) = (Vec3::new(1.0, 2.0, 3.0), Quat::from_rotation_x(0.4));
    for i in 0..10 {
        sections.push(Section {
            origin,
            orientation,
            radius: 0.5 - 0.02 * i as f32,
        });
        origin += orientation * Vec3::Y;
        orientation = orientation * Quat::from_rotation_z(0.15) * Quat::from_rotation_y(0.1);
    }
    let skeleton = Skeleton {
        branches: vec![procgena_tree::SkeletonBranch {
            level: 0,
            parent: None,
            sections,
            segment_count: 8,
            base_radius: 0.5,
        }],
        ..Skeleton::default()
    };
    let mut options = TreeOptions::default();
    options.bark.uv_mode = BarkUvMode::Continuous;
    options.bark.texture_scale.x = 4.0;

    for section_stride in [1, 3] {
        // Open tubes: cap triangles span one ring, where v is constant, so they
        // have no texture direction to follow.
        let detail = LodDetail {
            section_stride,
            closed: false,
            ..LodDetail::default()
        };
        let bark = skeleton.mesh(&options, &detail).branches;
        for &tri in bark.indices.as_chunks::<3>().0 {
            let p = tri.map(|i| Vec3::from_array(bark.positions[i as usize]));
            let uv = tri.map(|i| bark.uvs[i as usize]);
            // Derive the directions of increasing u and v from the triangle
            // edges and their texture-coordinate changes.
            let (e1, e2) = (p[1] - p[0], p[2] - p[0]);
            let (du1, dv1) = (uv[1][0] - uv[0][0], uv[1][1] - uv[0][1]);
            let (du2, dv2) = (uv[2][0] - uv[0][0], uv[2][1] - uv[0][1]);
            let det = du1 * dv2 - du2 * dv1;
            let t_tri = ((e1 * dv2 - e2 * dv1) / det).normalize();
            let b_tri = ((e2 * du1 - e1 * du2) / det).normalize();
            for &i in &tri {
                let t = bark.tangents[i as usize];
                let n = Vec3::from_array(bark.normals[i as usize]);
                let xyz = Vec3::new(t[0], t[1], t[2]);
                assert!(xyz.dot(t_tri) > 0.5, "tangent points away from +u");
                assert!(
                    n.cross(xyz).dot(b_tri) * t[3] > 0.5,
                    "bitangent points away from +v"
                );
            }
        }
    }
}

/// Checks every preset at the farthest level: thin branches are omitted, the
/// trunk and every fourth leaf remain, the crossed fallback shapes match the
/// shapes they stand in for, and the triangles drawn fall by more than half.
#[test]
fn farthest_level_culls_thin_branches_and_keeps_leaves() {
    let [.., far, farthest] = default_lod_levels();
    for preset in TreePreset::ALL {
        let options = preset.options();
        let skeleton = Skeleton::generate(&options).unwrap();
        let mesh = skeleton.mesh(&options, &farthest.detail);
        let name = preset.name();

        // Keep the trunk and omit exactly those other branches below the radius threshold.
        assert!(!mesh.branch_spans[0].indices.is_empty(), "{name}");
        for (span, branch) in mesh.branch_spans.iter().zip(&skeleton.branches) {
            let thin = branch.level > 0 && branch.base_radius < farthest.detail.min_branch_radius;
            assert_eq!(
                span.indices.is_empty(),
                thin,
                "{name} branch {}",
                span.branch
            );
        }

        // Branch removal must not reduce the number of leaves selected by leaf_stride.
        let kept = kept_leaves(&skeleton, farthest.detail.leaf_stride as usize);
        assert_eq!(mesh.leaf_spans.len(), kept.len(), "{name}");

        // The crossed fallback shapes pair each bark ribbon and fruit hexagon
        // one to one, and single leaf cards need none.
        let cull = &mesh.billboard_cull;
        let triangles = |buffers: &MeshBuffers| buffers.triangle_count();
        assert_eq!(
            triangles(&cull.branches),
            triangles(&mesh.branches),
            "{name}"
        );
        assert_eq!(triangles(&cull.fruit), triangles(&mesh.fruit), "{name}");
        assert_eq!(triangles(&cull.leaves), 0, "{name}");

        // Compare what a renderer that turns billboards toward the camera,
        // such as procgena_tree_bevy, draws. It never draws the crossed
        // fallback shapes, which only renderers without camera-facing
        // billboards add.
        let drawn = |mesh: &TreeMesh| mesh.triangle_count() - mesh.billboard_cull.triangle_count();
        let before = drawn(&skeleton.mesh(&options, &far.detail));
        let after = drawn(&mesh);
        assert!(after * 2 < before, "{name}: {after} vs {before} triangles");
    }
}

/// Returns triangles of `bark` with vertices merged where positions are
/// bitwise equal, so the texture seam and branch joins share vertices.
fn welded_triangles(bark: &MeshBuffers) -> Vec<[u32; 3]> {
    let mut ids = std::collections::HashMap::new();
    let welded: Vec<u32> = bark
        .positions
        .iter()
        .map(|p| {
            let next = ids.len() as u32;
            *ids.entry(p.map(f32::to_bits)).or_insert(next)
        })
        .collect();
    bark.indices
        .as_chunks::<3>()
        .0
        .iter()
        .map(|t| t.map(|i| welded[i as usize]))
        .collect()
}

/// Returns how many directed edges have no matching reverse edge, plus how
/// many appear more than once. Both are 0 for a closed, consistently wound mesh.
fn unpaired_edges(triangles: &[[u32; 3]]) -> (usize, usize) {
    let mut count = std::collections::HashMap::<(u32, u32), u32>::new();
    for &[a, b, c] in triangles {
        for e in [(a, b), (b, c), (c, a)] {
            *count.entry(e).or_default() += 1;
        }
    }
    let unmatched = count
        .keys()
        .filter(|&&(a, b)| !count.contains_key(&(b, a)))
        .count();
    let repeated = count.values().filter(|&&n| n > 1).count();
    (unmatched, repeated)
}

/// Groups welded triangles into edge-connected shells and returns the signed
/// volume each one encloses, positive when its triangles face outward.
fn shell_volumes(bark: &MeshBuffers, triangles: &[[u32; 3]]) -> Vec<f64> {
    // Union triangles that share a welded vertex.
    let vertices = triangles
        .iter()
        .flatten()
        .max()
        .map_or(0, |&m| m as usize + 1);
    let mut root: Vec<usize> = (0..vertices).collect();
    fn find(root: &mut [usize], mut i: usize) -> usize {
        while root[i] != i {
            root[i] = root[root[i]];
            i = root[i];
        }
        i
    }
    for &[a, b, c] in triangles {
        let ra = find(&mut root, a as usize);
        for v in [b, c] {
            let rv = find(&mut root, v as usize);
            root[rv] = ra;
        }
    }

    // Sum each shell's signed tetrahedron volumes about a point inside the tree.
    let first_position: std::collections::HashMap<u32, [f32; 3]> = {
        let mut map = std::collections::HashMap::new();
        let welded = welded_triangles(bark);
        for (t, w) in bark.indices.as_chunks::<3>().0.iter().zip(&welded) {
            for k in 0..3 {
                map.entry(w[k]).or_insert(bark.positions[t[k] as usize]);
            }
        }
        map
    };
    let mut volumes = std::collections::HashMap::<usize, f64>::new();
    for t in triangles {
        let [a, b, c] = t.map(|v| {
            let p = first_position[&v];
            glam::DVec3::new(p[0].into(), p[1].into(), p[2].into())
        });
        *volumes.entry(find(&mut root, t[0] as usize)).or_default() += a.dot(b.cross(c)) / 6.0;
    }
    volumes.into_values().collect()
}

/// Checks that closed bark is watertight and outward-facing for every preset
/// at the gameplay levels (LOD0, LOD1, and a similar level with thin branches
/// omitted).
#[test]
fn closed_bark_is_watertight_and_faces_outward() {
    let [full, near, ..] = default_lod_levels().map(|l| l.detail);
    let thinned = LodDetail {
        section_stride: 2,
        segment_factor: 0.6,
        min_branch_radius: 0.01,
        ..LodDetail::default()
    };
    for preset in TreePreset::ALL {
        let options = preset.options();
        let skeleton = Skeleton::generate(&options).unwrap();
        for (level, detail) in [full, near, thinned].iter().enumerate() {
            assert!(detail.closed);
            let what = format!("{} detail {level}", preset.name());
            let bark = skeleton.mesh(&options, detail).branches;
            let triangles = welded_triangles(&bark);
            assert_eq!(unpaired_edges(&triangles), (0, 0), "{what}");

            // No triangle may collapse to a line or point.
            for t in bark.indices.as_chunks::<3>().0 {
                let [a, b, c] = t.map(|i| Vec3::from_array(bark.positions[i as usize]));
                assert!(
                    (b - a).cross(c - a).length() > 0.0,
                    "{what}: zero-area {t:?}"
                );
            }
            for volume in shell_volumes(&bark, &triangles) {
                assert!(volume > 0.0, "{what}: shell volume {volume}");
            }
        }
    }
}

/// Checks that open bark leaves tube ends unsealed.
#[test]
fn open_bark_has_unsealed_ends() {
    let options = TreeOptions::default();
    let skeleton = Skeleton::generate(&options).unwrap();
    let open = LodDetail {
        closed: false,
        ..LodDetail::default()
    };
    let bark = skeleton.mesh(&options, &open).branches;
    assert!(unpaired_edges(&welded_triangles(&bark)).0 > 0);
}
