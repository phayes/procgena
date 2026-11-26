"""Builds the demo's lumberjack, `lumberjack.glb`, from the Quaternius CC0 packs.

Run it from the repository root whenever the source packs change:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P \
        demo/crates/procgena_demo_lumberjack/tools/prepare_lumberjack.py -- \
        demo/assets/extracted demo/assets/lumberjack/lumberjack.glb

The first argument is the folder the packs were extracted into (see
docs/demo-lumberjack-plan.md, "Downloaded and checked"); the second is the
file to write. The result is one skinned mesh on the shared 65-bone rig: the
male peasant outfit, the base character's head, eyes, beard, eyebrows and
hair, and the clips listed in CLIPS from the Universal Animation Libraries.
"""

import os
import sys

import bmesh
import bpy

args = sys.argv[sys.argv.index("--") + 1 :]
ROOT, OUT = os.path.abspath(args[0]), os.path.abspath(args[1])

BASE = ROOT + "/universal-base-characters/Universal Base Characters[Standard]/Base Characters/Godot - UE/"
HAIR = ROOT + "/universal-base-characters/Universal Base Characters[Standard]/Hairstyles/Rigged to Head Bone/glTF (Godot -Unreal)/"
OUTFIT = ROOT + "/modular-character-outfits-fantasy/Modular Character Outfits - Fantasy[Standard]/Exports/glTF (Godot-Unreal)/Outfits/Male_Peasant.gltf"
# Clips without root motion: the character controller moves the body.
UAL1 = ROOT + "/universal-animation-library/Universal Animation Library[Standard]/Unreal-Godot/UAL1_Standard.glb"
UAL2 = ROOT + "/universal-animation-library-2/Universal Animation Library 2[Standard]/Unreal-Godot/UAL2_Standard.glb"

# The clips the demo plays, by their names in the libraries.
CLIPS = [
    "Idle_Loop",
    "Walk_Loop",
    "Jog_Fwd_Loop",
    "Sprint_Loop",
    "Jump_Start",
    "Jump_Loop",
    "Jump_Land",
    "TreeChopping_Loop",
]

# Bones whose weight keeps a vertex of the base character: its head and neck.
HEAD_BONES = {"head", "neck_01", "neck_02"}


def import_gltf(path):
    """Imports a glTF file and returns the objects it added."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    return [o for o in bpy.data.objects if o not in before]


def fix_image_paths():
    """Points images at files that exist.

    The base characters' glTF names two normal maps `*_png.png`, but the
    files are named without `_png`, and the rigged hair names textures that
    ship only beside the base characters.
    """
    for image in bpy.data.images:
        path = bpy.path.abspath(image.filepath)
        if not image.filepath or os.path.exists(path):
            continue
        name = os.path.basename(path).replace("_png.png", ".png")
        for folder in (os.path.dirname(path) + "/", BASE):
            if os.path.exists(folder + name):
                image.filepath = folder + name
                image.reload()
                break
        else:
            raise SystemExit(f"missing texture: {image.filepath}")


def keep_head_only(mesh):
    """Deletes the vertices of `mesh` mostly weighted below the neck."""
    names = {g.index: g.name.lower() for g in mesh.vertex_groups}
    bm = bmesh.new()
    bm.from_mesh(mesh.data)
    deform = bm.verts.layers.deform.active
    drop = []
    for v in bm.verts:
        weights = v[deform] if deform else {}
        total = sum(weights.values()) or 1.0
        head = sum(w for i, w in weights.items() if names.get(i, "") in HEAD_BONES)
        if head / total < 0.5:
            drop.append(v)
    bmesh.ops.delete(bm, geom=drop, context="VERTS")
    bm.to_mesh(mesh.data)
    bm.free()


def delete(objects):
    for o in objects:
        bpy.data.objects.remove(o, do_unlink=True)


bpy.ops.wm.read_factory_settings(use_empty=True)

# The clips: import both libraries, keep the chosen actions, drop the rest.
for library in (UAL1, UAL2):
    delete(import_gltf(library))
for action in list(bpy.data.actions):
    if action.name in CLIPS:
        action.use_fake_user = True
    else:
        bpy.data.actions.remove(action)
missing = [c for c in CLIPS if c not in bpy.data.actions]
if missing:
    raise SystemExit(f"clips not found: {missing}")

# The body: the peasant outfit and its armature.
outfit = import_gltf(OUTFIT)
armature = next(o for o in outfit if o.type == "ARMATURE")
armature.name = "Lumberjack"
meshes = [o for o in outfit if o.type == "MESH" and o.parent == armature]

# The head from the base character, and the hair: moved onto the outfit's
# armature (their head bones rest where the outfit's do).
parts = import_gltf(BASE + "Superhero_Male_FullBody.gltf")
for name in ("Hair_Beard", "Eyebrows_Regular", "Hair_SimpleParted"):
    parts += import_gltf(HAIR + name + ".gltf")
for o in parts:
    if o.type != "MESH" or o.parent is None or o.parent.type != "ARMATURE":
        continue
    if o.name.startswith("SuperHero_Male"):
        keep_head_only(o)
    # The base character's own eyebrows are replaced by the hair pack's.
    if o.name == "Eyebrows":
        continue
    world = o.matrix_world.copy()
    o.parent = armature
    o.matrix_world = world
    for modifier in o.modifiers:
        if modifier.type == "ARMATURE":
            modifier.object = armature
    meshes.append(o)
delete([o for o in bpy.data.objects if o != armature and o not in meshes])
fix_image_paths()

# One skinned mesh.
bpy.ops.object.select_all(action="DESELECT")
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
bpy.ops.object.join()
meshes[0].name = "Lumberjack_Body"
meshes[0].data.name = "Lumberjack_Body"

# Every chosen clip, exported as an animation of the armature.
armature.animation_data_create()
os.makedirs(os.path.dirname(OUT), exist_ok=True)
bpy.ops.export_scene.gltf(
    filepath=OUT,
    export_format="GLB",
    export_animation_mode="ACTIONS",
    export_force_sampling=True,
    export_image_format="AUTO",
)
print("wrote", OUT)
