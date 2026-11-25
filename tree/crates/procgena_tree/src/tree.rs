//! Generate a tree from settings or a preset, and access its shape and meshes.

use procgena_core::chain::Lod;

use crate::{
    LodDetail, OptionsError, Rig, RigError, RigOptions, Skeleton, TreeMesh, TreeOptions, TreePreset,
};

/// A generated tree with settings, a branch and leaf layout, and triangles for rendering.
///
/// Create a tree from custom [`TreeOptions`], a built-in [`TreePreset`], or JSON
/// settings. Use [`Tree::mesh`] to access its game-level geometry (its real
/// shape, at [`TreeOptions::game_level`]), [`Tree::lower_mesh`] for the
/// coarser mesh drawn when it is small on screen, or [`Tree::mesh_with_detail`]
/// for any other detail.
/// Your application supplies materials, textures, and rendering.
///
/// To change the settings, clone [`Tree::options`], edit the clone, and pass it
/// to [`Tree::new`] to generate a new tree.
///
/// Generate an ash tree and check that its lower mesh uses fewer triangles:
///
/// ```
/// use procgena_tree::{Tree, TreePreset};
///
/// let tree = Tree::from_preset(TreePreset::ash_small())?;
/// assert!(tree.lower_mesh().triangle_count() < tree.triangle_count());
/// # Ok::<(), procgena_tree::OptionsError>(())
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Tree {
    options: TreeOptions,
    skeleton: Skeleton,
    mesh: TreeMesh,
}

impl Tree {
    /// Generates a tree and its game-level mesh from the supplied settings.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] if the branch depth exceeds the supported limit
    /// or a used branch level has zero sections along its length. See
    /// [`TreeOptions::validate`] for the checks performed; other values, such as
    /// dimensions, are not validated.
    pub fn new(options: TreeOptions) -> Result<Self, OptionsError> {
        let skeleton = Skeleton::generate(&options)?;
        let mesh = skeleton.mesh(&options, &options.game_level);
        Ok(Self {
            options,
            skeleton,
            mesh,
        })
    }

    /// Generates a tree and its game-level mesh from a preset's settings.
    ///
    /// # Errors
    ///
    /// Returns an [`OptionsError`] as [`Tree::new`] does. Built-in presets,
    /// and presets read from files, always pass.
    pub fn from_preset(preset: &TreePreset) -> Result<Self, OptionsError> {
        Self::new(preset.tree.clone())
    }

    /// Borrows the settings used to generate this tree.
    ///
    /// Clone them to make changes, then pass the edited settings to [`Tree::new`].
    pub fn options(&self) -> &TreeOptions {
        &self.options
    }

    /// Borrows the tree's skeleton: its branch shapes and leaf positions.
    ///
    /// Use this to inspect the tree's layout; use [`Tree::mesh`] for rendering geometry.
    pub fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    /// Borrows the game-level mesh, containing bark, leaf, fruit and flower
    /// triangles for rendering: the tree's real shape, built at
    /// [`TreeOptions::game_level`].
    pub fn mesh(&self) -> &TreeMesh {
        &self.mesh
    }

    /// The details of the tree's two meshes: its
    /// [`game_level`](TreeOptions::game_level), and the coarser detail derived
    /// from it.
    pub fn lod(&self) -> Lod<LodDetail> {
        Lod::from_game(self.options.game_level)
    }

    /// Creates the tree's lower mesh: the coarser mesh drawn in place of the
    /// game level when the tree is small on screen.
    pub fn lower_mesh(&self) -> TreeMesh {
        self.mesh_with_detail(&self.lod().lower)
    }

    /// Creates a mesh with the amount of branch and leaf detail specified by `detail`.
    ///
    /// Use [`LodDetail::default`] for full detail, or adjust it to simplify the
    /// geometry. The stored skeleton and game-level mesh remain unchanged.
    pub fn mesh_with_detail(&self, detail: &LodDetail) -> TreeMesh {
        self.skeleton.mesh(&self.options, detail)
    }

    /// Creates movable joints for this tree within the limits in `options`.
    ///
    /// Call [`Rig::skin`] with [`Tree::mesh`] or another mesh made from this
    /// tree to assign vertices to the joints. A renderer can then bend the tree by
    /// moving those joints; this method does not create animation clips.
    ///
    /// # Errors
    ///
    /// Returns [`RigError`] if the joint limit is invalid or fixed joint spacing
    /// exceeds it. See [`Rig::new`] for the allowed settings.
    pub fn rig(&self, options: &RigOptions) -> Result<Rig, RigError> {
        Rig::new(&self.skeleton, options)
    }

    /// Returns the total number of vertices (points) in the game-level mesh.
    ///
    /// Duplicate points are counted separately.
    pub fn vertex_count(&self) -> usize {
        self.mesh.vertex_count()
    }

    /// Returns the total number of triangles in the game-level mesh.
    pub fn triangle_count(&self) -> usize {
        self.mesh.triangle_count()
    }

    /// Consumes the tree and returns its settings, branch and leaf layout, and
    /// game-level mesh, in that order, transferring ownership to the caller.
    pub fn into_parts(self) -> (TreeOptions, Skeleton, TreeMesh) {
        (self.options, self.skeleton, self.mesh)
    }
}
