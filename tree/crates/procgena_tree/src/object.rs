//! Describe a generated tree as a tagged [`Object`] that other generators,
//! such as procgena_clump, can grow on.

use std::collections::BTreeMap;

use procgena_mesh::{Level, Object, Part, SurfaceMesh, Tag, TagSet, tags};

use crate::mesh::{BranchShape, BranchSpan, LodDetail, LodLevel, MeshBuffers, TreeMesh};
use crate::skeleton::Skeleton;
use crate::tree::Tree;

impl Tree {
    /// Returns the tree as a tagged [`Object`] with one detail level per entry
    /// of `lods`, for generators that grow on it, such as procgena_clump.
    ///
    /// Pass the result of [`Tree::lod_meshes`], or any meshes made from this
    /// tree with [`Tree::mesh_with_detail`], ordered by increasing distance.
    /// The first level starts at distance 0 and each later one at its
    /// [`LodLevel::distance`]. See [`TreeMesh::parts`] for the parts of each
    /// level.
    ///
    /// # Panics
    ///
    /// Panics if `lods` is empty, or if a mesh was not made from this tree.
    pub fn object(&self, lods: &[(LodLevel, TreeMesh)]) -> Object {
        self.skeleton().object(lods)
    }
}

impl Skeleton {
    /// Same as [`Tree::object`], for meshes made from this skeleton with
    /// [`Skeleton::mesh`].
    ///
    /// # Panics
    ///
    /// Panics if `lods` is empty, or if a mesh was not made from this skeleton.
    pub fn object(&self, lods: &[(LodLevel, TreeMesh)]) -> Object {
        assert!(!lods.is_empty(), "a tree object needs at least one level");
        let levels = lods
            .iter()
            .enumerate()
            .map(|(i, (lod, mesh))| Level {
                start: if i == 0 { 0.0 } else { lod.distance },
                parts: mesh.parts(self, &lod.detail),
            })
            .collect();
        Object { levels }
    }
}

impl TreeMesh {
    /// Returns the mesh as tagged parts, for generators that grow on it.
    ///
    /// `skeleton` must be the one this mesh was made from, and `detail` the
    /// settings it was made with.
    ///
    /// Bark is split into up to three parts by the role each branch plays:
    /// - `bark` + `trunk`: the trunk, and the branches that carry on from its tip;
    /// - `bark` + `limb`: branches growing from the side of the trunk, and the
    ///   branches that carry on from their tips;
    /// - `bark` + `branch`: every other branch.
    ///
    /// Each bark part lists one shell per chain of branches that carry on
    /// from each other's tips, and is closed when `detail` asks for closed
    /// tube bark ([`LodDetail::closed`]).
    ///
    /// Leaves are one open part tagged `leaf` + `card`; fruit and flowers
    /// are tagged `fruit` and `flower`. Empty parts are left out, and so is
    /// geometry without stored normals: ribbon bark
    /// ([`BranchShape::Ribbons`]) and camera-facing fruit
    /// ([`LodDetail::billboard_fruit`]).
    ///
    /// # Panics
    ///
    /// Panics if the mesh's branch spans don't match `skeleton`, or its
    /// buffers are inconsistent.
    pub fn parts(&self, skeleton: &Skeleton, detail: &LodDetail) -> Vec<Part> {
        let mut parts = Vec::new();

        // Bark, one part per branch role.
        if has_normals(&self.branches) {
            let chains = chains(skeleton);
            let closed = detail.closed && detail.branch_shape == BranchShape::Tube;
            for role in [Role::Trunk, Role::Limb, Role::Branch] {
                parts.extend(bark_part(self, &chains, role, closed));
            }
        }

        // Leaves, fruit and flowers, each as one whole buffer.
        parts.extend(whole_part(&self.leaves, [tags::LEAF, tags::CARD]));
        parts.extend(whole_part(&self.fruit, [tags::FRUIT]));
        parts.extend(whole_part(&self.flowers, [tags::FLOWER]));
        parts
    }
}

/// The part of the tree's shape a branch belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Trunk,
    Limb,
    Branch,
}

impl Role {
    /// Returns the tag naming this role.
    fn tag(self) -> Tag {
        match self {
            Self::Trunk => tags::TRUNK,
            Self::Limb => tags::LIMB,
            Self::Branch => tags::BRANCH,
        }
    }
}

/// Where one skeleton branch belongs: its role, and the index of the first
/// branch of its chain of tip continuations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Chain {
    role: Role,
    root: usize,
}

/// Returns each skeleton branch's role and chain, in branch order.
///
/// A branch that carries on from its parent's tip continues the parent's
/// chain and role. A branch growing from the side of a trunk chain starts a
/// limb, and any other side branch starts a branch.
fn chains(skeleton: &Skeleton) -> Vec<Chain> {
    let mut chains: Vec<Chain> = Vec::with_capacity(skeleton.branches.len());
    for (i, branch) in skeleton.branches.iter().enumerate() {
        let chain = match branch.parent {
            None => Chain {
                role: Role::Trunk,
                root: i,
            },
            Some(at) => {
                // Parents come before their children in the skeleton.
                let parent = chains[at.branch];
                let tip = skeleton.branches[at.branch].sections.len() - 1;
                if at.section == tip && at.t == 0.0 {
                    parent
                } else if parent.role == Role::Trunk {
                    Chain {
                        role: Role::Limb,
                        root: i,
                    }
                } else {
                    Chain {
                        role: Role::Branch,
                        root: i,
                    }
                }
            }
        };
        chains.push(chain);
    }
    chains
}

/// Copies the bark of every branch with `role` into one part, chain by
/// chain, so each chain becomes one shell. Returns `None` if no such branch
/// has triangles.
fn bark_part(mesh: &TreeMesh, chains: &[Chain], role: Role, closed: bool) -> Option<Part> {
    // Group the role's spans by chain, in order of each chain's first branch.
    let mut by_chain: BTreeMap<usize, Vec<&BranchSpan>> = BTreeMap::new();
    for span in &mesh.branch_spans {
        let chain = chains[span.branch];
        if chain.role == role {
            by_chain.entry(chain.root).or_default().push(span);
        }
    }

    // Copy each chain's spans next to each other and record its triangles.
    let mut out = SurfaceMesh::default();
    let mut shells = Vec::new();
    for spans in by_chain.values() {
        let first = (out.indices.len() / 3) as u32;
        for span in spans {
            append_span(&mut out, &mesh.branches, span);
        }
        let end = (out.indices.len() / 3) as u32;
        if end > first {
            shells.push(first..end);
        }
    }
    if out.indices.is_empty() {
        return None;
    }
    Some(Part {
        closed,
        shells,
        ..Part::new([tags::BARK, role.tag()], out)
    })
}

/// Appends one branch's vertices and triangles from `source` to `out`.
fn append_span(out: &mut SurfaceMesh, source: &MeshBuffers, span: &BranchSpan) {
    let base = out.positions.len() as u32;
    let v = span.vertices.start as usize..span.vertices.end as usize;
    out.positions
        .extend_from_slice(&source.positions[v.clone()]);
    out.normals.extend_from_slice(&source.normals[v]);

    // A branch's triangles only use its own vertices; shift them to the copies.
    let i = span.indices.start as usize..span.indices.end as usize;
    out.indices.extend(
        source.indices[i]
            .iter()
            .map(|&index| index - span.vertices.start + base),
    );
}

/// Wraps a whole buffer as an open part with `tags`. Returns `None` if it has
/// no triangles or no stored normals.
fn whole_part(buffers: &MeshBuffers, tags: impl Into<TagSet>) -> Option<Part> {
    if buffers.indices.is_empty() || !has_normals(buffers) {
        return None;
    }
    let mesh = SurfaceMesh {
        positions: buffers.positions.clone(),
        normals: buffers.normals.clone(),
        indices: buffers.indices.clone(),
    };
    Some(Part::new(tags, mesh))
}

/// Returns true if `buffers` stores one normal per position.
fn has_normals(buffers: &MeshBuffers) -> bool {
    buffers.normals.len() == buffers.positions.len()
}
