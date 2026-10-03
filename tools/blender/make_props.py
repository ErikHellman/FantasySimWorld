"""Generates the few props the KayKit collection lacks at character scale.

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_props.py

Writes one GLB per model into tools/blender/out/. Models follow KayKit
conventions: metres-ish units with a villager about 2.4 tall, origin at
ground level in the middle, chunky bevelled shapes, flat colours.
`pnpm assets` then bundles them with the rest.
"""
import math
import os
import random

import bpy

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out")
os.makedirs(OUT, exist_ok=True)

# Colours picked to sit beside the KayKit palette.
PALETTE = {
    "wood": (0.44, 0.27, 0.16),
    "wood_dark": (0.32, 0.19, 0.11),
    "wood_light": (0.60, 0.42, 0.26),
    "stem": (0.36, 0.55, 0.20),
    "leaf": (0.45, 0.66, 0.22),
    "petal_red": (0.86, 0.25, 0.22),
    "petal_yellow": (0.96, 0.78, 0.22),
    "petal_white": (0.95, 0.93, 0.86),
    "petal_violet": (0.60, 0.42, 0.82),
    "petal_blue": (0.36, 0.56, 0.90),
    "centre": (0.98, 0.70, 0.16),
    "cap_red": (0.80, 0.22, 0.18),
    "cap_brown": (0.62, 0.40, 0.24),
    "mushroom_stem": (0.93, 0.88, 0.76),
    "paper": (0.93, 0.88, 0.74),
    "rope": (0.72, 0.60, 0.40),
}

_materials = {}


def material(name):
    if name not in _materials:
        mat = bpy.data.materials.new("custom_" + name)
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        r, g, b = PALETTE[name]
        bsdf.inputs["Base Color"].default_value = (r, g, b, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.6
        bsdf.inputs["Metallic"].default_value = 0.0
        _materials[name] = mat
    return _materials[name]


def reset():
    bpy.ops.object.select_all(action="SELECT")
    bpy.ops.object.delete()


def finish(obj, colour, bevel=0.03):
    obj.data.materials.append(material(colour))
    if bevel > 0:
        modifier = obj.modifiers.new("bevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 1
        modifier.limit_method = "ANGLE"
    for polygon in obj.data.polygons:
        polygon.use_smooth = False
    return obj


def box(size, location, colour, rotation=(0, 0, 0), bevel=0.03):
    # Blender is z-up; the glTF exporter converts to y-up.
    bpy.ops.mesh.primitive_cube_add(size=1, location=location, rotation=rotation)
    obj = bpy.context.active_object
    obj.scale = size
    return finish(obj, colour, bevel)


def cylinder(radius, depth, location, colour, vertices=8, rotation=(0, 0, 0), top_scale=1.0, bevel=0.0):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius, radius2=radius * top_scale, depth=depth, location=location, rotation=rotation)
    return finish(bpy.context.active_object, colour, bevel)


def dome(radius, location, colour, squash=0.6, vertices=8):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=vertices, ring_count=4, radius=radius, location=location)
    obj = bpy.context.active_object
    obj.scale = (1, 1, squash)
    return finish(obj, colour, 0)


def export(name):
    bpy.ops.object.select_all(action="SELECT")
    for obj in bpy.context.selected_objects:
        bpy.context.view_layer.objects.active = obj
    bpy.ops.object.convert(target="MESH")
    bpy.ops.object.join()
    obj = bpy.context.active_object
    obj.name = name
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    bpy.ops.export_scene.gltf(filepath=os.path.join(OUT, name + ".glb"), export_format="GLB", use_selection=True, export_apply=True, export_yup=True)
    print("wrote", name)


def fence(name, length=4.0, broken=False):
    """A rustic post-and-rail fence segment running along x."""
    reset()
    rng = random.Random(name)
    posts = [-length / 2, 0.0, length / 2]
    for index, x in enumerate(posts):
        height = 1.25 + rng.uniform(-0.06, 0.1)
        box((0.24, 0.24, height), (x, 0, height / 2), "wood_dark", rotation=(0, 0, rng.uniform(-0.2, 0.2)))
        box((0.3, 0.3, 0.08), (x, 0, height + 0.02), "wood", rotation=(0, 0, rng.uniform(-0.3, 0.3)))
        if index == len(posts) - 1:
            break
        centre = (x + posts[index + 1]) / 2
        span = posts[index + 1] - x
        for rail, z in enumerate((0.45, 0.92)):
            if broken and index == 1 and rail == 1:
                box((span * 0.55, 0.1, 0.16), (centre - span * 0.2, 0.02, z - 0.18), "wood_light", rotation=(0, 0.35, 0.05))
                continue
            box((span + 0.1, 0.1, 0.16), (centre, rng.uniform(-0.02, 0.02), z + rng.uniform(-0.03, 0.03)), "wood" if rail else "wood_light", rotation=(0, rng.uniform(-0.03, 0.03), 0))
    export(name)


def fence_post(name):
    reset()
    box((0.26, 0.26, 1.3), (0, 0, 0.65), "wood_dark")
    box((0.32, 0.32, 0.08), (0, 0, 1.32), "wood")
    export(name)


def flowers(name, petals, count, seed):
    """A small clump of wild flowers."""
    reset()
    rng = random.Random(seed)
    for _ in range(count):
        angle = rng.uniform(0, math.tau)
        distance = rng.uniform(0.05, 0.38)
        x, y = math.cos(angle) * distance, math.sin(angle) * distance
        height = rng.uniform(0.32, 0.62)
        lean = (rng.uniform(-0.18, 0.18), rng.uniform(-0.18, 0.18), 0)
        cylinder(0.022, height, (x, y, height / 2), "stem", vertices=5, rotation=lean)
        top = (x - lean[1] * height * 0.5, y + lean[0] * height * 0.5, height)
        colour = rng.choice(petals)
        for petal in range(5):
            a = petal / 5 * math.tau
            dome(0.07, (top[0] + math.cos(a) * 0.075, top[1] + math.sin(a) * 0.075, top[2]), colour, squash=0.45, vertices=6)
        dome(0.055, (top[0], top[1], top[2] + 0.025), "centre", squash=0.7, vertices=6)
        # A leaf or two near the ground.
        for _leaf in range(rng.randint(1, 2)):
            a = rng.uniform(0, math.tau)
            box((0.2, 0.07, 0.015), (x + math.cos(a) * 0.1, y + math.sin(a) * 0.1, 0.08), "leaf", rotation=(0, -0.5, a), bevel=0.0)
    export(name)


def mushrooms(name, cap, count, seed):
    """A cluster of toadstools."""
    reset()
    rng = random.Random(seed)
    for index in range(count):
        angle = rng.uniform(0, math.tau)
        distance = 0 if index == 0 else rng.uniform(0.18, 0.4)
        x, y = math.cos(angle) * distance, math.sin(angle) * distance
        scale = 1.0 if index == 0 else rng.uniform(0.45, 0.8)
        stem = 0.34 * scale
        cylinder(0.075 * scale, stem, (x, y, stem / 2), "mushroom_stem", vertices=7, top_scale=0.75)
        dome(0.2 * scale, (x, y, stem + 0.02 * scale), cap, squash=0.62, vertices=8)
        if cap == "cap_red":
            for _spot in range(3):
                a = rng.uniform(0, math.tau)
                r = 0.1 * scale
                dome(0.035 * scale, (x + math.cos(a) * r, y + math.sin(a) * r, stem + 0.105 * scale), "petal_white", squash=0.5, vertices=5)
    export(name)


def notice_board(name):
    """A village notice board with a little roof and pinned papers."""
    reset()
    for x in (-0.75, 0.75):
        box((0.16, 0.16, 2.1), (x, 0, 1.05), "wood_dark")
    box((1.5, 0.08, 0.95), (0, 0, 1.35), "wood_light")
    box((1.9, 0.5, 0.08), (0, 0.12, 2.22), "wood", rotation=(0.35, 0, 0))
    box((1.9, 0.5, 0.08), (0, -0.12, 2.22), "wood", rotation=(-0.35, 0, 0))
    rng = random.Random(name)
    for x, z in ((-0.42, 1.5), (0.05, 1.42), (0.45, 1.56), (-0.2, 1.12), (0.35, 1.12)):
        box((0.3, 0.02, 0.36), (x, 0.055, z), "paper", rotation=(0, rng.uniform(-0.12, 0.12), 0), bevel=0.0)
    export(name)


def banner_board(name):
    """A plank board on two posts for a KayKit banner to hang on.

    The banners are made for walls: the cloth has no back faces and hangs just in
    front of glTF z = 0.378 (Blender y = -0.378). The board's front sits right
    behind it so the banner looks nailed on and is solid from behind.
    """
    reset()
    rng = random.Random(name)
    planks = 4
    width = 1.7
    for index in range(planks):
        x = -width / 2 + (index + 0.5) * width / planks
        top = 3.95 + rng.uniform(-0.06, 0.06)
        bottom = 0.35 + rng.uniform(-0.04, 0.04)
        box((width / planks - 0.015, 0.1, top - bottom), (x, -0.31, (top + bottom) / 2), "wood_light" if index % 2 else "wood", rotation=(0, rng.uniform(-0.015, 0.015), 0))
    for x in (-0.6, 0.6):
        box((0.2, 0.2, 4.15), (x, -0.16, 2.075), "wood_dark", rotation=(0, 0, rng.uniform(-0.1, 0.1)))
    box((1.95, 0.1, 0.22), (0, -0.01, 3.55), "wood", rotation=(0, rng.uniform(-0.02, 0.02), 0))
    box((1.95, 0.1, 0.22), (0, -0.01, 0.85), "wood", rotation=(0, rng.uniform(-0.02, 0.02), 0))
    export(name)


fence("fence_wood")
fence("fence_wood_broken", broken=True)
fence_post("fence_wood_post")
flowers("flowers_red", ["petal_red", "petal_red", "petal_white"], 5, 1)
flowers("flowers_yellow", ["petal_yellow", "petal_yellow", "petal_white"], 5, 2)
flowers("flowers_violet", ["petal_violet", "petal_blue", "petal_white"], 6, 3)
flowers("flowers_mixed", ["petal_red", "petal_yellow", "petal_violet", "petal_blue", "petal_white"], 7, 4)
mushrooms("mushrooms_red", "cap_red", 3, 5)
mushrooms("mushrooms_brown", "cap_brown", 4, 6)
notice_board("notice_board")
banner_board("banner_board")
