"""Renders review sheets of the animals that make_animals.py writes.

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/render_animals_preview.py -- <out dir> [sheets...]

Sheets: lineup (every species beside a villager, to scale), ride (a villager on a
horse), scene (a small farmyard
from about the game camera's height), tiles (each model alone, and each species
from four sides; ImageMagick's `montage` then joins them into variants.png and
turnarounds.png). With no sheet names, all of them.

The game draws with no tone mapping, so the renders use Blender's Standard view
transform to show the palette as it will look.
"""
import math
import os
import shutil
import subprocess
import sys

import bpy
from mathutils import Quaternion, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
ANIMALS = os.path.join(HERE, "out_animals")
PROPS = os.path.join(HERE, "out")
FARMER = os.path.join(REPO, "public/assets/characters/Farmer_A.glb")
FARMER_TEXTURE = os.path.join(REPO, "public/assets/textures/farmer_texture_A.png")

args = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
OUT = os.path.abspath(args[0] if args else os.path.join(HERE, "preview"))
SHEETS = set(args[1:]) or {"lineup", "ride", "scene", "tiles"}
os.makedirs(OUT, exist_ok=True)

# One of each species, smallest first, then the other colours.
SPECIES = ["robin", "crow", "chicken_white", "duck_white", "owl_brown", "cat_ginger", "dog_brown", "pig_pink", "sheep_white", "goat_white", "cow_holstein", "horse_chestnut"]
ALL = [
    "dog_brown", "dog_collie", "cat_ginger", "cat_grey", "pig_pink", "pig_spotted", "sheep_white", "sheep_black",
    "goat_white", "goat_brown", "cow_holstein", "cow_brown", "horse_chestnut", "horse_grey", "chicken_white", "chicken_brown",
    "duck_white", "duck_mallard", "owl_brown", "owl_snowy", "robin", "bluebird", "robin_flying", "crow", "crow_flying",
]


def linear(hex_colour):
    value = int(hex_colour.lstrip("#"), 16)

    def channel(shift):
        c = ((value >> shift) & 255) / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    return (channel(16), channel(8), channel(0), 1.0)


def flat_material(name, hex_colour, roughness=0.9):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    bsdf = mat.node_tree.nodes.get("Principled BSDF")
    bsdf.inputs["Base Color"].default_value = linear(hex_colour)
    bsdf.inputs["Roughness"].default_value = roughness
    return mat


def stage(ground="#8cb562", sky="#ccd4df", size=(1024, 1024), samples=48):
    """A fresh scene: ground, warm sun, blue sky light."""
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "BLENDER_EEVEE"
    scene.render.resolution_x, scene.render.resolution_y = size
    scene.render.resolution_percentage = 100
    scene.view_settings.view_transform = "Standard"
    scene.eevee.taa_render_samples = samples
    if hasattr(scene.eevee, "use_raytracing"):
        scene.eevee.use_raytracing = True
    world = bpy.data.worlds.new("sky")
    world.use_nodes = True
    background = world.node_tree.nodes["Background"]
    background.inputs["Color"].default_value = linear(sky)
    background.inputs["Strength"].default_value = 0.9
    scene.world = world

    sun = bpy.data.objects.new("sun", bpy.data.lights.new("sun", "SUN"))
    sun.data.energy = 3.2
    sun.data.color = (1.0, 0.95, 0.86)
    sun.data.angle = math.radians(6)
    sun.rotation_euler = (math.radians(42), 0, math.radians(-30))
    scene.collection.objects.link(sun)

    bpy.ops.mesh.primitive_plane_add(size=200, location=(0, 0, 0))
    bpy.context.active_object.data.materials.append(flat_material("ground", ground))

    camera = bpy.data.objects.new("camera", bpy.data.cameras.new("camera"))
    scene.collection.objects.link(camera)
    scene.camera = camera
    return scene, camera


def load(path, location=(0, 0, 0), turn=0.0):
    """Imports a GLB under one empty, so it can be placed and removed as a whole."""
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    holder = bpy.data.objects.new(os.path.basename(path), None)
    bpy.context.scene.collection.objects.link(holder)
    for obj in new:
        # The processed KayKit characters carry a stray unit sphere; it is not part of the model.
        if obj.name.startswith("Icosphere"):
            bpy.data.objects.remove(obj, do_unlink=True)
        elif obj.parent is None:
            obj.parent = holder
    holder.location = location
    holder.rotation_euler = (0, 0, turn)
    return holder


def descendants(obj):
    out = []
    for child in obj.children:
        out.append(child)
        out.extend(descendants(child))
    return out


def remove(holder):
    for obj in descendants(holder) + [holder]:
        bpy.data.objects.remove(obj, do_unlink=True)


def farmer(location, turn=0.0, riding=False):
    """Farmer_A with the arms lowered from the rest T-pose, or sitting astride with `riding`, for scale.

    The processed models leave their palette texture out (the client adds it), so it is put back here.
    """
    holder = load(FARMER, location, turn)
    image = bpy.data.images.load(FARMER_TEXTURE, check_existing=True)
    for obj in descendants(holder):
        for slot in getattr(obj, "material_slots", ()):
            tree = slot.material.node_tree
            texture = tree.nodes.new("ShaderNodeTexImage")
            texture.image = image
            texture.interpolation = "Closest"
            tree.links.new(texture.outputs["Color"], tree.nodes["Principled BSDF"].inputs["Base Color"])
    rig = next(o for o in descendants(holder) if o.type == "ARMATURE")
    # Turns in the rig's own axes: x across the body, y front to back (forward is -y), z up.
    turns = {"upperarm.l": Quaternion((0, 1, 0), 1.25), "upperarm.r": Quaternion((0, 1, 0), -1.25)}
    if riding:
        turns["upperarm.l"] = Quaternion((1, 0, 0), -0.5) @ Quaternion((0, 1, 0), 1.1)
        turns["upperarm.r"] = Quaternion((1, 0, 0), -0.5) @ Quaternion((0, 1, 0), -1.1)
        # The legs are too short to wrap round a horse: thighs go out sideways over its back
        # and the shins hang straight down its flanks.
        for side, s in (("l", 1), ("r", -1)):
            turns["upperleg." + side] = Quaternion((0, 1, 0), -s * 1.25) @ Quaternion((1, 0, 0), -0.3)
            turns["lowerleg." + side] = Quaternion((0, 1, 0), s * 1.2)
    for bone, turn_by in turns.items():
        pose = rig.pose.bones[bone]
        rest = pose.bone.matrix_local.to_quaternion()
        pose.rotation_quaternion = rest.inverted() @ turn_by @ rest
    return holder


def seat_of(holder):
    """Where a rider's hips go on a horse: its empty named seat."""
    bpy.context.view_layer.update()
    return next(o for o in descendants(holder) if o.name.startswith("seat")).matrix_world.translation.copy()


def bounds(holders):
    bpy.context.view_layer.update()
    depsgraph = bpy.context.evaluated_depsgraph_get()
    corners = []
    for holder in holders:
        for obj in descendants(holder):
            if obj.type == "MESH":
                evaluated = obj.evaluated_get(depsgraph)
                corners.extend(evaluated.matrix_world @ Vector(c) for c in evaluated.bound_box)
    return corners


def aim(camera, target, azimuth, elevation, distance=30.0):
    """Puts the camera on a sphere round target; azimuth 0 looks at an animal's face, 90 at its left side."""
    direction = Vector((math.sin(math.radians(azimuth)), -math.cos(math.radians(azimuth)), 0)) * math.cos(math.radians(elevation))
    direction.z = math.sin(math.radians(elevation))
    camera.location = Vector(target) + direction * distance
    camera.rotation_mode = "QUATERNION"
    camera.rotation_quaternion = (-direction).to_track_quat("-Z", "Y")
    bpy.context.view_layer.update()


def frame(camera, holders, azimuth, elevation, margin=1.12):
    """An orthographic view that fits the models."""
    corners = bounds(holders)
    centre = sum(corners, Vector()) / len(corners)
    aim(camera, centre, azimuth, elevation)
    inverse = camera.matrix_world.inverted()
    local = [inverse @ c for c in corners]
    xs, ys = [p.x for p in local], [p.y for p in local]
    right, up = camera.matrix_world.to_3x3() @ Vector((1, 0, 0)), camera.matrix_world.to_3x3() @ Vector((0, 1, 0))
    camera.location += right * (min(xs) + max(xs)) / 2 + up * (min(ys) + max(ys)) / 2
    scene = bpy.context.scene
    aspect = scene.render.resolution_x / scene.render.resolution_y
    camera.data.type = "ORTHO"
    camera.data.ortho_scale = max(max(xs) - min(xs), (max(ys) - min(ys)) * aspect) * margin
    camera.data.clip_end = 200


def render(path):
    bpy.context.scene.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print("rendered", path)


def label(text, location, size=0.16):
    """An upright caption facing the front camera."""
    curve = bpy.data.curves.new(text, "FONT")
    curve.body = text
    curve.size = size
    curve.align_x = "CENTER"
    obj = bpy.data.objects.new(text, curve)
    obj.location = location
    obj.rotation_euler = (math.radians(90), 0, 0)
    obj.data.materials.append(flat_material("label", "#3a3330"))
    bpy.context.scene.collection.objects.link(obj)


def lineup():
    scene, camera = stage(ground="#d8d0bd", size=(2400, 900), samples=64)
    x = 0.0
    holders = []
    for name in SPECIES:
        holder = load(os.path.join(ANIMALS, name + ".glb"))
        width = max(c.x for c in bounds([holder])) - min(c.x for c in bounds([holder]))
        x += width / 2
        holder.location.x = x
        label(name.split("_")[0], (x, -1.3, 0.02))
        x += width / 2 + 0.3
        holders.append(holder)
    holder = farmer((x + 0.6, 0, 0))
    label("villager", (x + 0.6, -1.3, 0.02))
    holders.append(holder)
    frame(camera, holders, azimuth=28, elevation=16, margin=1.1)
    render(os.path.join(OUT, "lineup.png"))


def farmyard():
    scene, camera = stage(size=(1920, 1200), samples=64)
    fence = os.path.join(PROPS, "fence_wood.glb")
    # A paddock 12 wide and 8 deep, open at the front-left for a gate.
    for x in (-4, 0, 4):
        load(fence, (x, 4, 0))
    for x in (0, 4):
        load(fence, (x, -4, 0))
    for y in (-2, 2):
        load(fence, (-6, y, 0), turn=math.pi / 2)
        load(fence, (6, y, 0), turn=math.pi / 2)
    load(os.path.join(PROPS, "flowers_mixed.glb"), (-2.6, -7.4, 0))
    load(os.path.join(PROPS, "flowers_yellow.glb"), (5.2, -5.6, 0))
    load(os.path.join(PROPS, "mushrooms_red.glb"), (7.4, 1.0, 0))
    placed = [
        ("horse_chestnut", (3.3, 1.8), 2.4), ("horse_grey", (0.6, 1.9), -1.2), ("cow_holstein", (-2.8, 1.6), 0.6), ("cow_brown", (-4.2, -0.6), 2.0),
        ("sheep_white", (0.4, -0.4), -0.5), ("sheep_white", (1.4, -1.2), 0.3), ("sheep_black", (-0.6, -1.4), 0.9),
        ("goat_brown", (4.4, -1.6), -1.0), ("goat_white", (2.8, -2.6), 0.5), ("pig_pink", (-2.6, -2.4), -0.8), ("pig_spotted", (-1.2, -2.0), 0.4),
        ("chicken_white", (-6.4, -5.6), 0.8), ("chicken_brown", (-5.5, -6.3), -0.4), ("chicken_white", (-6.9, -6.6), 2.6),
        ("duck_mallard", (2.0, -5.9), 0.3), ("duck_white", (2.6, -6.4), 0.2), ("duck_white", (1.4, -6.5), -0.2),
        ("dog_collie", (-1.0, -5.2), 0.5), ("cat_ginger", (-7.6, -2.4), -1.2), ("cat_grey", (6.6, -4.2), 0.7), ("dog_brown", (-7.4, -4.6), -0.6),
        ("owl_brown", (6.0, 4.0), -0.3),
    ]
    loaded = {}
    for name, (x, y), turn in placed:
        loaded[name] = load(os.path.join(ANIMALS, name + ".glb"), (x, y, 0), turn)
    rider_turn = dict((name, turn) for name, _, turn in placed)["horse_grey"]
    farmer(seat_of(loaded["horse_grey"]) - Vector((0, 0, 0.38)), turn=rider_turn, riding=True)
    # The owl and the robin perch on fence posts; posts are about 1.3 tall.
    for holder in bpy.data.objects:
        if holder.name == "owl_brown.glb":
            holder.location.z = 1.3
    load(os.path.join(ANIMALS, "robin.glb"), (-6.0, 0.0, 1.32), 1.4)
    load(os.path.join(ANIMALS, "bluebird.glb"), (4.0, -4.0, 1.32), -0.4)
    load(os.path.join(ANIMALS, "crow_flying.glb"), (-2.0, 1.0, 4.4), 0.9)
    load(os.path.join(ANIMALS, "robin_flying.glb"), (3.6, -3.0, 3.2), -0.6)
    load(os.path.join(ANIMALS, "crow.glb"), (0.0, 4.0, 1.32), 0.2)
    farmer((-4.0, -4.4, 0), turn=-0.2)

    camera.data.type = "PERSP"
    camera.data.lens = 40
    aim(camera, (0, -1.5, 0.6), azimuth=14, elevation=46, distance=21)
    render(os.path.join(OUT, "in_scene.png"))


def ride():
    scene, camera = stage(ground="#d8d0bd", size=(1800, 1100), samples=64)
    mounted = load(os.path.join(ANIMALS, "horse_chestnut.glb"), (0, 0, 0))
    # The rig's hips sit 0.41 above its origin; the mesh's seat is a little lower.
    rider = farmer(seat_of(mounted) - Vector((0, 0, 0.38)), riding=True)
    saddled = load(os.path.join(ANIMALS, "horse_grey.glb"), (2.4, 0.6, 0))
    standing = farmer((-1.7, -0.2, 0))
    frame(camera, [mounted, rider, saddled, standing], azimuth=38, elevation=14, margin=1.08)
    render(os.path.join(OUT, "ride.png"))


def tiles():
    tile_dir = os.path.join(OUT, "tiles")
    shutil.rmtree(tile_dir, ignore_errors=True)
    os.makedirs(tile_dir)
    scene, camera = stage(ground="#d8d0bd", size=(420, 420), samples=32)
    variants, turnarounds = [], []
    for name in ALL:
        holder = load(os.path.join(ANIMALS, name + ".glb"))
        if name.endswith("_flying"):
            holder.location.z = 0.5
        frame(camera, [holder], azimuth=32, elevation=24)
        path = os.path.join(tile_dir, f"{name}.png")
        render(path)
        variants.append((path, name.replace("_", " ")))
        remove(holder)
    for name in SPECIES:
        holder = load(os.path.join(ANIMALS, name + ".glb"))
        for view, azimuth in (("front", 0), ("three-quarter", 35), ("side", 90), ("back", 180)):
            frame(camera, [holder], azimuth=azimuth, elevation=14)
            path = os.path.join(tile_dir, f"{name}_{view}.png")
            render(path)
            turnarounds.append((path, f"{name.split('_')[0]}, {view}"))
        remove(holder)

    montage = shutil.which("montage") or "/opt/homebrew/bin/montage"
    # ImageMagick has no default font on macOS; any system TrueType face will do for captions.
    font = next((f for f in ("/System/Library/Fonts/Supplemental/Arial.ttf", "/System/Library/Fonts/Helvetica.ttc") if os.path.exists(f)), None)
    common = ["-background", "#efe9dc", "-fill", "#3a3330", "-pointsize", "20", "-geometry", "+6+6", "-depth", "8"] + (["-font", font] if font else [])

    def labelled(entries):
        return [arg for path, caption in entries for arg in ("-label", caption, path)]

    subprocess.run([montage, *labelled(variants), "-tile", "6x", *common, os.path.join(OUT, "variants.png")], check=True)
    subprocess.run([montage, *labelled(turnarounds), "-tile", "8x", "-resize", "300x300", *common, os.path.join(OUT, "turnarounds.png")], check=True)


if "lineup" in SHEETS:
    lineup()
if "ride" in SHEETS:
    ride()
if "scene" in SHEETS:
    farmyard()
if "tiles" in SHEETS:
    tiles()
