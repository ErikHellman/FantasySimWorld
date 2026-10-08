"""Generates KayKit-style pets, farm animals and birds.

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_animals.py [-- dog cat ...]

Writes one GLB per animal and colour into tools/blender/out_animals/ (names after
`--` build only the models whose names start with them); `pnpm assets` bundles
them as `animals`.

The conventions are make_props.py's: a villager is about 2.4 tall, the origin is
on the ground under the middle of the body, and shapes are chunky bevelled blocks
with flat shading and flat colours. Proportions follow the KayKit characters: big
heads, stubby legs and tall black oval eyes. Animals face -y in Blender, which the
glTF exporter turns into +z, the way the KayKit characters face.

Each animal is a tree of named joints (body, head, leg_fl, ear_l, tail, wing_l, ...),
each an empty at the joint with its mesh hung under it, so the game animates them
by turning the joints (src/render/animals.ts). An animal's left is +x.
"""
import math
import os
import sys

import bpy
from mathutils import Euler, Matrix, Vector

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "out_animals")
os.makedirs(OUT, exist_ok=True)

EYE = "#1e1b1f"
WHITE = "#f7f4ee"
NOSE = "#2b2522"


def linear(hex_colour):
    """Blender colours are linear; the palette is written in sRGB like the KayKit textures."""
    value = int(hex_colour.lstrip("#"), 16)

    def channel(shift):
        c = ((value >> shift) & 255) / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    return (channel(16), channel(8), channel(0))


_materials = {}


def material(hex_colour):
    if hex_colour not in _materials:
        mat = bpy.data.materials.new("animal_" + hex_colour.lstrip("#"))
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        bsdf.inputs["Base Color"].default_value = (*linear(hex_colour), 1.0)
        # Eyes get a little shine, the way KayKit's eyes catch the light.
        bsdf.inputs["Roughness"].default_value = 0.3 if hex_colour == EYE else 0.6
        bsdf.inputs["Metallic"].default_value = 0.0
        mat.diffuse_color = (*linear(hex_colour), 1.0)
        _materials[hex_colour] = mat
    return _materials[hex_colour]


def reset():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for mesh in list(bpy.data.meshes):
        bpy.data.meshes.remove(mesh)


# Pieces made since the last call to part(); part() joins them into one mesh.
_pieces = []


def _finish(obj, colour, bevel=0.0):
    obj.data.materials.append(material(colour))
    if bevel > 0:
        modifier = obj.modifiers.new("bevel", "BEVEL")
        modifier.width = bevel
        modifier.segments = 2
        modifier.limit_method = "NONE"
    for polygon in obj.data.polygons:
        polygon.use_smooth = False
    _pieces.append(obj)
    return obj


def _place(obj, location, rotation=(0, 0, 0), scale=(1, 1, 1)):
    # The transform goes into the mesh so the bevel is the same width on every side.
    obj.data.transform(Matrix.LocRotScale(Vector(location), Euler(rotation), Vector(scale)))


def box(size, location, colour, rotation=(0, 0, 0), round=0.4):
    """A block whose edges are rounded off by `round` of half its smallest side."""
    bpy.ops.mesh.primitive_cube_add(size=1, location=(0, 0, 0))
    obj = bpy.context.active_object
    _place(obj, location, rotation, size)
    return _finish(obj, colour, round * min(size) / 2)


def blob(size, location, colour, rotation=(0, 0, 0), segments=8, rings=6):
    """A faceted ellipsoid `size` across."""
    bpy.ops.mesh.primitive_uv_sphere_add(segments=segments, ring_count=rings, radius=0.5, location=(0, 0, 0))
    obj = bpy.context.active_object
    _place(obj, location, rotation, size)
    return _finish(obj, colour)


def rod(base, tip, radius, colour, tip_radius=None, vertices=6):
    """A tapered prism from base to tip: legs, tails, horns, beaks."""
    base, tip = Vector(base), Vector(tip)
    axis = tip - base
    bpy.ops.mesh.primitive_cone_add(
        vertices=vertices,
        radius1=radius,
        radius2=radius if tip_radius is None else tip_radius,
        depth=axis.length,
        location=(0, 0, 0),
    )
    obj = bpy.context.active_object
    rotation = Vector((0, 0, 1)).rotation_difference(axis.normalized())
    obj.data.transform(Matrix.LocRotScale((base + tip) / 2, rotation, None))
    return _finish(obj, colour)


def ring(major, minor, location, colour, rotation=(0, 0, 0)):
    bpy.ops.mesh.primitive_torus_add(major_radius=major, minor_radius=minor, major_segments=8, minor_segments=4)
    obj = bpy.context.active_object
    obj.location = (0, 0, 0)
    obj.rotation_euler = (0, 0, 0)
    _place(obj, location, rotation)
    return _finish(obj, colour)


def eyes(x, y, z, size, side=False, whites=False):
    """A pair of KayKit eyes: tall black ovals on the face, or on the sides of the head with `side`.

    `whites` sets them on white, for dark faces where black on its own would vanish.
    """
    for s in (1, -1):
        dims = (size * 0.4, size * 0.68, size) if side else (size * 0.68, size * 0.4, size)
        if whites:
            blob(tuple(d * 1.5 for d in dims), (s * x, y, z), WHITE)
            out = Vector((s * size * 0.12, 0, 0)) if side else Vector((0, -size * 0.12, 0))
            blob(dims, Vector((s * x, y, z)) + out, EYE)
        else:
            blob(dims, (s * x, y, z), EYE)


def root(name):
    obj = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(obj)
    obj["pivot"] = (0.0, 0.0, 0.0)
    return obj


def part(name, pivot, parent):
    """Joins the pieces made since the last part into one mesh hung under an empty at the joint `pivot`.

    The joint is an empty, not the mesh itself, because the asset build quantizes
    meshes: gltf-transform folds the de-quantisation into the transform of any mesh
    node without children, which would move a leg's hip or an ear's root. The empty
    keeps the name the game turns (`leg_fl`); the mesh under it is `leg_fl_mesh`.
    """
    pieces = list(_pieces)
    _pieces.clear()
    bpy.ops.object.select_all(action="DESELECT")
    for piece in pieces:
        piece.select_set(True)
    bpy.context.view_layer.objects.active = pieces[0]
    bpy.ops.object.convert(target="MESH")
    if len(pieces) > 1:
        bpy.ops.object.join()
    mesh = bpy.context.view_layer.objects.active
    mesh.name = name + "_mesh"
    mesh.data.name = name
    pivot = Vector(pivot)
    mesh.data.transform(Matrix.Translation(-pivot))
    joint = bpy.data.objects.new(name, None)
    bpy.context.scene.collection.objects.link(joint)
    joint["pivot"] = tuple(pivot)
    joint.parent = parent
    joint.location = pivot - Vector(parent["pivot"])
    mesh.parent = joint
    mesh.location = (0, 0, 0)
    return joint


def export(name):
    bpy.ops.object.select_all(action="SELECT")
    for obj in bpy.context.selected_objects:
        if "pivot" in obj:
            del obj["pivot"]
    bpy.ops.export_scene.gltf(
        filepath=os.path.join(OUT, name + ".glb"),
        export_format="GLB",
        use_selection=True,
        export_apply=True,
        export_yup=True,
    )
    print("wrote", name)


SIDES = (("l", 1), ("r", -1))


def legs4(body, width, length, top, thickness, colour, foot_colour, foot=0.08, inset=0.04, socks=()):
    """Four straight legs from inside the body down to the ground, each a part turning at the hip."""
    x = width / 2 - thickness / 2 - inset
    y = length / 2 - thickness / 2 - inset
    for name, sx, sy in (("fl", 1, -1), ("fr", -1, -1), ("bl", 1, 1), ("br", -1, 1)):
        px, py = sx * x, sy * y
        box((thickness, thickness, top), (px, py, top / 2), colour, round=0.3)
        if name in socks:
            box((thickness * 1.06, thickness * 1.06, top * 0.38), (px, py, top * 0.19 + foot / 2), WHITE, round=0.3)
        box((thickness * 1.12, thickness * 1.2, foot), (px, py - thickness * 0.06, foot / 2), foot_colour, round=0.35)
        part("leg_" + name, (px, py, top), body)


def bird_legs(body, x, y, top, colour, toe=0.1, radius=0.022, webbed=False):
    """Two thin legs with three toes, or a webbed foot, turning at the hip."""
    for side, s in SIDES:
        rod((s * x, y, top), (s * x, y, 0.02), radius, colour, vertices=5)
        if webbed:
            box((toe * 0.9, toe * 1.1, 0.025), (s * x, y - toe * 0.4, 0.0125), colour, round=0.5)
        else:
            for angle in (-0.5, 0, 0.5):
                d = Vector((math.sin(angle), -math.cos(angle), 0)) * toe * 0.45
                box((0.03, toe, 0.025), (s * x + d.x, y + d.y, 0.0125), colour, rotation=(0, 0, -angle), round=0.5)
        part("leg_" + side, (s * x, y, top), body)


# Pets ------------------------------------------------------------------------------------------------------


def dog(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.24, 0.4, 0.62, 0.34
    bz = leg + bh / 2 - 0.02
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.55)
    box((bw * 0.66, 0.1, bh * 0.66), (0, -bl / 2 + 0.03, bz - 0.03), c["light"], round=0.5)
    if c.get("saddle"):
        box((bw + 0.02, bl * 0.5, bh * 0.45), (0, 0.06, bz + bh * 0.3), c["saddle"], round=0.6)
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.06, 0.13, c["coat"], c["paw"], inset=0.03)

    hw, hl, hh = 0.44, 0.38, 0.38
    hy, hz = -bl / 2 - 0.06, bz + bh / 2 + 0.14
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["coat"], round=0.55)
    if c.get("blaze"):
        box((0.1, 0.04, hh * 0.7), (0, front + 0.005, hz + 0.02), c["light"], round=0.5)
    box((0.26, 0.2, 0.16), (0, front - 0.05, hz - 0.08), c["light"], round=0.55)
    box((0.1, 0.06, 0.07), (0, front - 0.16, hz - 0.03), NOSE, round=0.5)
    eyes(0.1, front, hz + 0.06, 0.085)
    head = part("head", (0, hy + 0.12, hz - 0.12), body)
    for side, s in SIDES:
        box((0.08, 0.16, 0.24), (s * (hw / 2 + 0.02), hy + 0.03, hz + 0.04), c["ear"], rotation=(0, -s * 0.25, 0), round=0.5)
        part("ear_" + side, (s * (hw / 2 - 0.01), hy + 0.03, hz + hh / 2 - 0.02), head)

    base = (0, bl / 2 - 0.05, bz + 0.08)
    rod(base, (0, bl / 2 + 0.12, bz + 0.3), 0.05, c["coat"], tip_radius=0.04)
    blob((0.09, 0.09, 0.1), (0, bl / 2 + 0.125, bz + 0.31), c["tail_tip"])
    part("tail", base, body)
    export(name)


def cat(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.17, 0.26, 0.46, 0.24
    bz = leg + bh / 2 - 0.02
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.55)
    box((bw * 0.62, 0.08, bh * 0.62), (0, -bl / 2 + 0.03, bz - 0.02), c["light"], round=0.5)
    for y in (-0.1, 0.02, 0.14):
        box((bw * 0.8, 0.045, 0.03), (0, y, bz + bh / 2 + 0.002), c["stripe"], round=0.5)
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.05, 0.09, c["coat"], c["paw"], foot=0.05, inset=0.02)

    hw, hl, hh = 0.36, 0.3, 0.3
    hy, hz = -bl / 2 - 0.04, bz + bh / 2 + 0.12
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["coat"], round=0.55)
    for x in (-0.06, 0.06):
        box((0.035, 0.08, 0.02), (x, hy - 0.02, hz + hh / 2 + 0.002), c["stripe"], round=0.5)
    box((0.15, 0.08, 0.09), (0, front - 0.02, hz - 0.07), c["light"], round=0.55)
    box((0.05, 0.03, 0.035), (0, front - 0.06, hz - 0.04), c["nose"], round=0.5)
    eyes(0.085, front, hz + 0.025, 0.075)
    head = part("head", (0, hy + 0.1, hz - 0.1), body)
    for side, s in SIDES:
        rod((s * 0.1, hy + 0.02, hz + hh / 2 - 0.04), (s * 0.135, hy + 0.02, hz + hh / 2 + 0.11), 0.075, c["coat"], tip_radius=0.0, vertices=4)
        part("ear_" + side, (s * 0.1, hy + 0.02, hz + hh / 2 - 0.04), head)

    base = (0, bl / 2 - 0.04, bz + 0.04)
    rod(base, (0, bl / 2 + 0.12, bz + 0.16), 0.035, c["coat"])
    rod((0, bl / 2 + 0.12, bz + 0.16), (0, bl / 2 + 0.15, bz + 0.38), 0.035, c["coat"])
    blob((0.075, 0.075, 0.09), (0, bl / 2 + 0.15, bz + 0.39), c["tail_tip"])
    part("tail", base, body)
    export(name)


# Farm animals -----------------------------------------------------------------------------------------------


def pig(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.16, 0.56, 0.78, 0.5
    bz = leg + bh / 2 - 0.04
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.8)
    for x, y, w, l in c.get("spots", ()):
        blob((w, l, 0.12), (x, y, bz + bh / 2 - 0.045), c["spot"])
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.08, 0.14, c["coat"], c["hoof"], foot=0.06, inset=0.06)

    hw, hl, hh = 0.5, 0.38, 0.44
    hy, hz = -bl / 2 - 0.02, bz + 0.12
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["coat"], round=0.7)
    rod((0, front + 0.03, hz - 0.06), (0, front - 0.09, hz - 0.06), 0.12, c["snout"], vertices=10)
    for s in (1, -1):
        blob((0.035, 0.02, 0.05), (s * 0.04, front - 0.092, hz - 0.06), c["nostril"])
    eyes(0.12, front, hz + 0.07, 0.08)
    head = part("head", (0, hy + 0.14, hz - 0.08), body)
    for side, s in SIDES:
        rod((s * 0.15, hy, hz + hh / 2 - 0.05), (s * 0.24, hy - 0.15, hz + hh / 2 + 0.05), 0.085, c["ear"], tip_radius=0.0, vertices=4)
        part("ear_" + side, (s * 0.15, hy, hz + hh / 2 - 0.05), head)

    ring(0.05, 0.02, (0, bl / 2 + 0.02, bz + 0.1), c["snout"], rotation=(math.pi / 2, 0, 0))
    part("tail", (0, bl / 2, bz + 0.1), body)
    export(name)


def sheep(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.28, 0.5, 0.7, 0.44
    bz = leg + bh / 2 - 0.06
    box((bw * 0.92, bl * 0.92, bh * 0.92), (0, 0, bz), c["wool"], round=0.7)
    # Lumpy fleece: rings of faceted blobs over the top and down the sides.
    for y in (-0.24, -0.08, 0.08, 0.24):
        for angle in (-1.7, -1.0, -0.35, 0.35, 1.0, 1.7):
            wobble = 0.02 * math.sin(y * 31 + angle * 7)
            blob(
                (0.24, 0.24, 0.22),
                (math.sin(angle) * (bw / 2 - 0.06), y + wobble, bz + math.cos(angle) * (bh / 2 - 0.06)),
                c["wool"],
                segments=6,
                rings=4,
            )
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.06, 0.1, c["face"], c["hoof"], foot=0.07, inset=0.07)

    hw, hl, hh = 0.28, 0.34, 0.32
    hy, hz = -bl / 2 - 0.06, bz + bh * 0.28
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["face"], round=0.6)
    for x, y in ((-0.06, 0.02), (0.06, 0.02), (0, -0.05), (0, 0.08)):
        blob((0.15, 0.15, 0.13), (x, hy + y, hz + hh / 2 + 0.01), c["wool"], segments=6, rings=4)
    eyes(0.07, front, hz + 0.03, 0.07, whites=True)
    head = part("head", (0, hy + 0.12, hz - 0.08), body)
    for side, s in SIDES:
        box((0.16, 0.08, 0.045), (s * (hw / 2 + 0.06), hy + 0.04, hz + 0.05), c["face"], rotation=(0, s * 0.35, 0), round=0.6)
        part("ear_" + side, (s * hw / 2, hy + 0.04, hz + 0.07), head)

    blob((0.16, 0.14, 0.16), (0, bl / 2 + 0.02, bz + 0.1), c["wool"], segments=6, rings=4)
    part("tail", (0, bl / 2 - 0.04, bz + 0.1), body)
    export(name)


def goat(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.36, 0.4, 0.7, 0.4
    bz = leg + bh / 2 - 0.04
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.6)
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.06, 0.11, c["coat"], c["hoof"], foot=0.08, inset=0.04)

    hw, hl, hh = 0.32, 0.38, 0.34
    hy, hz = -bl / 2 - 0.04, bz + bh / 2 + 0.16
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["coat"], round=0.6)
    box((0.2, 0.16, 0.18), (0, front - 0.04, hz - 0.08), c["muzzle"], round=0.6)
    box((0.08, 0.04, 0.05), (0, front - 0.12, hz - 0.04), NOSE, round=0.5)
    eyes(0.085, front, hz + 0.06, 0.075)
    for s in (1, -1):
        top = (s * 0.07, hy + 0.02, hz + hh / 2 - 0.03)
        bend = (s * 0.09, hy + 0.13, hz + hh / 2 + 0.13)
        rod(top, bend, 0.045, c["horn"], tip_radius=0.035)
        rod(bend, (s * 0.1, hy + 0.27, hz + hh / 2 + 0.1), 0.035, c["horn"], tip_radius=0.008)
    rod((0, front - 0.02, hz - 0.15), (0, front + 0.02, hz - 0.31), 0.05, c["beard"], tip_radius=0.0, vertices=5)
    head = part("head", (0, hy + 0.12, hz - 0.14), body)
    for side, s in SIDES:
        box((0.16, 0.07, 0.04), (s * (hw / 2 + 0.07), hy + 0.06, hz + 0.04), c["coat"], rotation=(0, s * 0.3, 0), round=0.6)
        part("ear_" + side, (s * hw / 2, hy + 0.06, hz + 0.06), head)

    base = (0, bl / 2 - 0.03, bz + bh * 0.3)
    rod(base, (0, bl / 2 + 0.06, bz + bh * 0.3 + 0.14), 0.05, c["coat"], tip_radius=0.02)
    part("tail", base, body)
    export(name)


def cow(name, c):
    reset()
    animal = root(name)
    leg, bw, bl, bh = 0.42, 0.74, 1.12, 0.66
    bz = leg + bh / 2 - 0.06
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.55)
    # Patches sit on the flat parts of the sides and back, clear of the rounded edges.
    for s, y, z, w, h in c.get("spots", ()):
        if s == 0:
            box((w, h, 0.03), (0, y, bz + bh / 2 + 0.002), c["spot"], round=0.7)
        else:
            box((0.03, w, h), (s * (bw / 2 + 0.002), y, bz + z), c["spot"], round=0.7)
    blob((0.26, 0.26, 0.16), (0, bl / 2 - 0.3, bz - bh / 2 + 0.01), c["udder"])
    for x, y in ((-0.05, -0.05), (0.05, -0.05), (-0.05, 0.05), (0.05, 0.05)):
        rod((x, bl / 2 - 0.3 + y, bz - bh / 2), (x, bl / 2 - 0.3 + y, bz - bh / 2 - 0.1), 0.018, c["udder"], vertices=5)
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.08, 0.18, c["legs"], c["hoof"], foot=0.1, inset=0.06)

    hw, hl, hh = 0.56, 0.46, 0.5
    hy, hz = -bl / 2 - 0.08, bz + bh / 2 + 0.06
    front = hy - hl / 2
    box((hw, hl, hh), (0, hy, hz), c["coat"], round=0.55)
    if c.get("head_spot"):
        box((0.22, 0.24, 0.03), (0.1, hy + 0.04, hz + hh / 2 + 0.002), c["spot"], round=0.7)
    box((0.48, 0.18, 0.24), (0, front - 0.05, hz - 0.14), c["muzzle"], round=0.6)
    for s in (1, -1):
        blob((0.06, 0.03, 0.08), (s * 0.1, front - 0.14, hz - 0.1), c["nostril"])
    eyes(0.14, front, hz + 0.07, 0.1)
    for s in (1, -1):
        rod((s * 0.2, hy + 0.05, hz + hh / 2 - 0.05), (s * 0.36, hy + 0.05, hz + hh / 2 + 0.1), 0.05, c["horn"], tip_radius=0.02)
    head = part("head", (0, hy + 0.16, hz - 0.12), body)
    for side, s in SIDES:
        box((0.2, 0.08, 0.11), (s * (hw / 2 + 0.08), hy + 0.06, hz + 0.04), c["ear"], rotation=(0, s * 0.3, 0), round=0.6)
        part("ear_" + side, (s * hw / 2, hy + 0.06, hz + 0.07), head)

    base = (0, bl / 2 - 0.02, bz + bh * 0.35)
    rod(base, (0, bl / 2 + 0.07, bz - 0.25), 0.03, c["coat"], vertices=5)
    blob((0.1, 0.1, 0.15), (0, bl / 2 + 0.075, bz - 0.3), c["tuft"])
    part("tail", base, body)
    export(name)


def horse(name, c):
    """A riding horse, sized for a villager to sit astride.

    The villagers are chibi, with legs only about half a unit long, so the horse
    is tall (a rider's feet hang well clear of the ground) but its back stays
    narrow enough for those short legs to straddle. The saddle is its own part
    so the game can show it only on horses that are ridden, and an empty named
    `seat` marks where a rider's hips go.
    """
    reset()
    animal = root(name)
    leg, bw, bl, bh = 1.0, 0.62, 1.6, 0.72
    bz = leg + bh / 2 - 0.06
    top = bz + bh / 2
    box((bw, bl, bh), (0, 0, bz), c["coat"], round=0.55)
    body = part("body", (0, 0, bz), animal)
    legs4(body, bw, bl, leg + 0.08, 0.21, c["coat"], c["hoof"], foot=0.15, inset=0.06, socks=c.get("socks", ()))

    # The neck belongs to the head so the two bow together. The head and neck are the
    # first draft's scaled up by k, which kept the face's proportions.
    k = 1.3
    lean = 0.45
    axis = Vector((0, -math.sin(lean), math.cos(lean)))
    back = Vector((0, math.cos(lean), math.sin(lean)))
    neck = Vector((0, -bl / 2 + 0.1 * k, top + 0.14 * k))
    box((0.3 * k, 0.34 * k, 0.66 * k), neck, c["coat"], rotation=(lean, 0, 0), round=0.5)
    for t in (-0.26, -0.12, 0.02, 0.16, 0.3):
        box((0.12 * k, 0.13 * k, 0.17 * k), neck + axis * t * k + back * 0.17 * k, c["mane"], rotation=(lean, 0, 0), round=0.5)
    tilt = 0.4
    along = Vector((0, -math.cos(tilt), -math.sin(tilt)))
    up = Vector((0, 0, k))
    head_centre = neck + (axis * 0.36 + along * 0.2) * k
    box((0.36 * k, 0.64 * k, 0.38 * k), head_centre, c["coat"], rotation=(tilt, 0, 0), round=0.55)
    box((0.34 * k, 0.2 * k, 0.3 * k), head_centre + along * 0.27 * k - up * 0.02, c["muzzle"], rotation=(tilt, 0, 0), round=0.55)
    if c.get("blaze"):
        box((0.1 * k, 0.4 * k, 0.03 * k), head_centre + along * 0.02 * k + up * 0.19, WHITE, rotation=(tilt, 0, 0), round=0.5)
    box((0.14 * k, 0.14 * k, 0.12 * k), head_centre - along * 0.18 * k + up * 0.2, c["mane"], rotation=(tilt, 0, 0), round=0.5)
    eye_at = head_centre - along * 0.05 * k + up * 0.07
    eyes(0.18 * k, eye_at.y, eye_at.z, 0.1 * k, side=True)
    head = part("head", (0, -bl / 2 + 0.12 * k, top - 0.08 * k), body)
    for side, s in SIDES:
        ear = head_centre - along * 0.24 * k + Vector((s * 0.1 * k, 0, 0.16 * k))
        rod(ear, ear + Vector((s * 0.03, 0.03, 0.17)) * k, 0.065 * k, c["coat"], tip_radius=0.0, vertices=4)
        part("ear_" + side, ear, head)

    base = Vector((0, bl / 2 - 0.03, bz + bh * 0.32))
    bend = base + Vector((0, 0.2, -0.16)) * k
    rod(base, bend, 0.09 * k, c["mane"], tip_radius=0.07 * k)
    rod(bend, bend + Vector((0, 0.05, -0.42)) * k, 0.075 * k, c["mane"], tip_radius=0.03 * k)
    part("tail", base, body)

    # A blanket wrapped over the back, a saddle on it and short stirrups: a villager's legs
    # are short, so their feet sit just below the saddle and out over the horse's sides.
    sy = 0.08
    wrap = 0.6 * bh
    box((bw + 0.03, 0.66, wrap), (0, sy, top - wrap / 2 + 0.025), c["blanket"], round=0.17 / (wrap / 2))
    box((bw * 0.62, 0.5, 0.1), (0, sy, top + 0.07), c["leather"], round=0.6)
    box((0.22, 0.1, 0.16), (0, sy - 0.24, top + 0.13), c["leather"], round=0.6)
    box((bw * 0.56, 0.09, 0.16), (0, sy + 0.24, top + 0.13), c["leather"], round=0.6)
    for s in (1, -1):
        stirrup = Vector((s * 0.4, sy - 0.12, top - 0.1))
        rod((s * bw * 0.28, sy - 0.06, top + 0.06), stirrup + Vector((0, 0, 0.02)), 0.02, c["leather"], vertices=4)
        box((0.14, 0.16, 0.04), stirrup, "#8d8a88", round=0.5)
    saddle = part("saddle", (0, sy, top), body)
    seat = bpy.data.objects.new("seat", None)
    bpy.context.scene.collection.objects.link(seat)
    seat.parent = saddle
    seat.location = (0, 0, 0.12)
    export(name)


# Birds -----------------------------------------------------------------------------------------------------


def folded_wings(body, x, y, z, size, colour):
    """Wings folded against the sides, each a part hinged at the shoulder."""
    for side, s in SIDES:
        blob(size, (s * x, y, z), colour)
        part("wing_" + side, (s * (x - size[0] / 2), y - size[1] * 0.35, z + size[2] * 0.3), body)


def spread_wings(body, x, y, z, span, chord, colour, tip_colour):
    """Wings held out flat for flight, each a part hinged at the shoulder so it can flap."""
    for side, s in SIDES:
        box((span * 0.6, chord, 0.035), (s * (x + span * 0.3), y, z), colour, round=0.6)
        box((span * 0.5, chord * 0.75, 0.03), (s * (x + span * 0.78), y + chord * 0.12, z + 0.01), tip_colour, rotation=(0, 0, s * 0.18), round=0.6)
        part("wing_" + side, (s * x, y, z), body)


def chicken(name, c):
    reset()
    animal = root(name)
    leg = 0.14
    bz = leg + 0.16
    blob((0.34, 0.44, 0.36), (0, 0.02, bz), c["feather"])
    for x, fan in ((-0.07, -0.45), (0, 0), (0.07, 0.45)):
        box((0.06, 0.13, 0.26), (x, 0.22, bz + 0.12), c["tail"], rotation=(-0.6, fan, 0), round=0.7)
    body = part("body", (0, 0, bz), animal)
    bird_legs(body, 0.08, 0.04, bz - 0.08, c["legs"])
    folded_wings(body, 0.16, 0.05, bz + 0.01, (0.07, 0.26, 0.18), c["wing"])

    hz = bz + 0.24
    blob((0.22, 0.22, 0.24), (0, -0.17, hz), c["feather"])
    for y, z, size in ((-0.24, 0.12, 0.08), (-0.17, 0.14, 0.09), (-0.1, 0.12, 0.08)):
        blob((0.05, size, size), (0, y, hz + z - 0.01), c["comb"])
    rod((0, -0.26, hz), (0, -0.35, hz - 0.02), 0.04, c["beak"], tip_radius=0.0, vertices=4)
    blob((0.05, 0.05, 0.08), (0, -0.27, hz - 0.08), c["comb"])
    eyes(0.1, -0.21, hz + 0.03, 0.055, side=True)
    part("head", (0, -0.12, bz + 0.1), body)
    export(name)


def duck(name, c):
    reset()
    animal = root(name)
    leg = 0.08
    bz = leg + 0.14
    blob((0.32, 0.48, 0.28), (0, 0.02, bz), c["body"])
    if c.get("breast"):
        blob((0.26, 0.16, 0.22), (0, -0.17, bz + 0.02), c["breast"])
    box((0.15, 0.14, 0.06), (0, 0.23, bz + 0.05), c["tail"], rotation=(-0.45, 0, 0), round=0.6)
    body = part("body", (0, 0, bz), animal)
    bird_legs(body, 0.07, 0.04, bz - 0.06, c["legs"], toe=0.11, webbed=True)
    folded_wings(body, 0.14, 0.06, bz + 0.03, (0.06, 0.3, 0.14), c["wing"])

    hz = bz + 0.24
    rod((0, -0.16, bz + 0.06), (0, -0.18, hz - 0.06), 0.065, c["head"], vertices=6)
    if c.get("collar"):
        ring(0.065, 0.016, (0, -0.172, bz + 0.15), c["collar"])
    blob((0.2, 0.22, 0.2), (0, -0.19, hz), c["head"])
    box((0.13, 0.15, 0.05), (0, -0.33, hz - 0.03), c["bill"], round=0.6)
    eyes(0.09, -0.23, hz + 0.03, 0.05, side=True)
    part("head", (0, -0.16, bz + 0.06), body)
    export(name)


def crow(name, c, flying=False):
    reset()
    animal = root(name)
    leg = 0.1
    bz = leg + 0.12
    blob((0.2, 0.36, 0.22), (0, 0.02, bz), c["body"])
    box((0.14, 0.22, 0.03), (0, 0.24, bz + 0.02), c["tail"], rotation=(0.25 if flying else -0.3, 0, 0), round=0.6)
    body = part("body", (0, 0, bz), animal)
    if flying:
        spread_wings(body, 0.08, -0.01, bz + 0.04, 0.5, 0.2, c["wing"], c["tip"])
    else:
        bird_legs(body, 0.05, 0.03, bz - 0.06, c["legs"], toe=0.08, radius=0.016)
        folded_wings(body, 0.09, 0.07, bz + 0.02, (0.05, 0.3, 0.14), c["wing"])

    hz = bz + 0.13
    blob((0.18, 0.2, 0.18), (0, -0.15, hz), c["body"])
    rod((0, -0.23, hz), (0, -0.36, hz - 0.02), 0.035, c["beak"], tip_radius=0.0, vertices=4)
    eyes(0.08, -0.18, hz + 0.03, 0.045, side=True, whites=True)
    part("head", (0, -0.1, bz + 0.05), body)
    export(name)


def songbird(name, c, flying=False):
    reset()
    animal = root(name)
    leg = 0.06
    bz = leg + 0.09
    blob((0.18, 0.24, 0.18), (0, 0.01, bz), c["back"])
    blob((0.15, 0.12, 0.14), (0, -0.07, bz - 0.01), c["breast"])
    box((0.09, 0.14, 0.025), (0, 0.16, bz + 0.05), c["back"], rotation=(-0.6 if not flying else 0.1, 0, 0), round=0.6)
    body = part("body", (0, 0, bz), animal)
    if flying:
        spread_wings(body, 0.06, 0.0, bz + 0.03, 0.26, 0.11, c["wing"], c["wing"])
    else:
        bird_legs(body, 0.035, 0.02, bz - 0.05, c["legs"], toe=0.05, radius=0.011)
        folded_wings(body, 0.075, 0.04, bz + 0.02, (0.04, 0.17, 0.1), c["wing"])

    hz = bz + 0.11
    blob((0.15, 0.15, 0.15), (0, -0.07, hz), c["back"])
    blob((0.11, 0.06, 0.08), (0, -0.12, hz - 0.035), c["breast"])
    rod((0, -0.14, hz), (0, -0.2, hz - 0.01), 0.02, c["beak"], tip_radius=0.0, vertices=4)
    eyes(0.065, -0.1, hz + 0.02, 0.035, side=True)
    part("head", (0, -0.05, bz + 0.04), body)
    export(name)


def owl(name, c):
    reset()
    animal = root(name)
    leg = 0.05
    bz = leg + 0.22
    blob((0.38, 0.32, 0.46), (0, 0, bz), c["body"])
    blob((0.28, 0.12, 0.34), (0, -0.12, bz - 0.03), c["belly"])
    for x, z in c.get("flecks", ()):
        box((0.05, 0.02, 0.03), (x, -0.185, bz + z), c["fleck"], round=0.5)
    box((0.16, 0.12, 0.04), (0, 0.17, bz - 0.17), c["wing"], rotation=(0.6, 0, 0), round=0.6)
    body = part("body", (0, 0, bz), animal)
    for side, s in SIDES:
        for dx in (-0.035, 0, 0.035):
            box((0.03, 0.08, 0.03), (s * 0.08 + dx, -0.04, 0.015), c["talon"], round=0.5)
        part("foot_" + side, (s * 0.08, 0, 0.03), body)
    folded_wings(body, 0.18, 0.03, bz + 0.02, (0.08, 0.26, 0.36), c["wing"])

    hz = bz + 0.34
    hy = -0.02
    box((0.42, 0.32, 0.32), (0, hy, hz), c["body"], round=0.7)
    for s in (1, -1):
        blob((0.18, 0.05, 0.19), (s * 0.085, hy - 0.15, hz), c["face"])
        blob((0.1, 0.03, 0.1), (s * 0.085, hy - 0.175, hz + 0.01), c["iris"])
        blob((0.055, 0.03, 0.06), (s * 0.085, hy - 0.19, hz + 0.01), EYE)
        if c.get("tufts", True):
            rod((s * 0.14, hy - 0.04, hz + 0.13), (s * 0.19, hy - 0.04, hz + 0.25), 0.05, c["body"], tip_radius=0.0, vertices=4)
    rod((0, hy - 0.17, hz - 0.03), (0, hy - 0.2, hz - 0.1), 0.03, c["beak"], tip_radius=0.0, vertices=4)
    part("head", (0, hy, hz - 0.14), body)
    export(name)


# The roster ------------------------------------------------------------------------------------------------

ROSTER = [
    (dog, "dog_brown", {"coat": "#b8834f", "light": "#f1dcb5", "ear": "#7a4f2c", "paw": "#f1dcb5", "tail_tip": "#b8834f"}),
    (dog, "dog_collie", {"coat": "#34302f", "light": "#f2efe8", "ear": "#34302f", "paw": "#f2efe8", "tail_tip": "#f2efe8", "blaze": True}),
    (cat, "cat_ginger", {"coat": "#e3893f", "light": "#f6e2c4", "stripe": "#b5602a", "nose": "#e58f9a", "paw": "#f6e2c4", "tail_tip": "#b5602a"}),
    (cat, "cat_grey", {"coat": "#8d8f99", "light": "#ebe8e2", "stripe": "#6a6c76", "nose": "#e58f9a", "paw": "#ebe8e2", "tail_tip": "#6a6c76"}),
    (chicken, "chicken_white", {"feather": "#f4f1ea", "wing": "#e6e1d6", "tail": "#e6e1d6", "comb": "#d9393a", "beak": "#f2b33a", "legs": "#eba43a"}),
    (chicken, "chicken_brown", {"feather": "#b06a32", "wing": "#8c4f22", "tail": "#3a2a24", "comb": "#d9393a", "beak": "#f2b33a", "legs": "#eba43a"}),
    (duck, "duck_white", {"body": "#f4f1ea", "head": "#f4f1ea", "wing": "#e6e1d6", "tail": "#e6e1d6", "bill": "#f2a23a", "legs": "#f08a2a"}),
    (duck, "duck_mallard", {"body": "#a38d72", "head": "#2f7a4a", "breast": "#7a4a36", "collar": WHITE, "wing": "#8d8a88", "tail": "#34302f", "bill": "#e6c23a", "legs": "#f08a2a"}),
    (pig, "pig_pink", {"coat": "#f0a8a5", "snout": "#e48a8e", "nostril": "#8a4b52", "ear": "#e48a8e", "hoof": "#8a5a50"}),
    (pig, "pig_spotted", {"coat": "#f0a8a5", "snout": "#e48a8e", "nostril": "#8a4b52", "ear": "#3a3232", "hoof": "#3a3232", "spot": "#3a3232", "spots": ((0.04, -0.12, 0.24, 0.26), (-0.05, 0.2, 0.2, 0.2))}),
    (sheep, "sheep_white", {"wool": "#f3ecdc", "face": "#3e3532", "hoof": "#26201e"}),
    (sheep, "sheep_black", {"wool": "#4d4643", "face": "#2b2523", "hoof": "#1c1816"}),
    (goat, "goat_white", {"coat": "#ece6da", "muzzle": "#dcd0c0", "horn": "#b9a98a", "beard": "#ece6da", "hoof": "#5a4a3c"}),
    (goat, "goat_brown", {"coat": "#9a6a43", "muzzle": "#e6d6bb", "horn": "#cdbd9b", "beard": "#5e3e26", "hoof": "#3a2a20"}),
    (cow, "cow_holstein", {"coat": "#f3f0ea", "legs": "#f3f0ea", "spot": "#2f2b2b", "muzzle": "#eab0ac", "nostril": "#8a4b52", "udder": "#eab0ac", "horn": "#ece0c2", "ear": "#2f2b2b", "hoof": "#3a3330", "tuft": "#2f2b2b", "head_spot": True,
                           "spots": ((1, -0.22, 0.04, 0.3, 0.2), (1, 0.24, -0.02, 0.24, 0.18), (-1, -0.05, 0.02, 0.36, 0.22), (-1, 0.3, 0.06, 0.18, 0.14), (0, 0.12, 0, 0.26, 0.3))}),
    (cow, "cow_brown", {"coat": "#9a5a35", "legs": "#9a5a35", "muzzle": "#e8c9a8", "nostril": "#6a3a2a", "udder": "#e8b9a8", "horn": "#ece0c2", "ear": "#9a5a35", "hoof": "#3a2a20", "tuft": "#5a3220"}),
    (horse, "horse_chestnut", {"coat": "#a65a2e", "mane": "#4a2c1c", "muzzle": "#7e4223", "hoof": "#3a3330", "blaze": True, "socks": ("fl", "br"), "blanket": "#b8413a", "leather": "#6a3f24"}),
    (horse, "horse_grey", {"coat": "#c9c6c2", "mane": "#f2f0ec", "muzzle": "#8d8a88", "hoof": "#4a4542", "blanket": "#3f6fae", "leather": "#4a3426"}),
    (crow, "crow", {"body": "#34333d", "wing": "#2a2932", "tip": "#22212a", "tail": "#2a2932", "beak": "#4a4854", "legs": "#4a4854"}),
    (lambda n, c: crow(n, c, flying=True), "crow_flying", {"body": "#34333d", "wing": "#2a2932", "tip": "#22212a", "tail": "#2a2932", "beak": "#4a4854", "legs": "#4a4854"}),
    (songbird, "robin", {"back": "#8a6a4a", "breast": "#e2703a", "wing": "#735638", "beak": "#3a3330", "legs": "#6a5a4a"}),
    (songbird, "bluebird", {"back": "#4a7ac8", "breast": "#e08a4a", "wing": "#3a62a8", "beak": "#3a3330", "legs": "#6a5a4a"}),
    (lambda n, c: songbird(n, c, flying=True), "robin_flying", {"back": "#8a6a4a", "breast": "#e2703a", "wing": "#735638", "beak": "#3a3330", "legs": "#6a5a4a"}),
    (lambda n, c: songbird(n, c, flying=True), "bluebird_flying", {"back": "#4a7ac8", "breast": "#e08a4a", "wing": "#3a62a8", "beak": "#3a3330", "legs": "#6a5a4a"}),
    (owl, "owl_brown", {"body": "#9a7050", "belly": "#e6d3b0", "wing": "#7a5438", "face": "#efe2c8", "iris": "#f2c23a", "beak": "#c9a96a", "talon": "#c9a96a", "fleck": "#9a7050",
                        "flecks": ((-0.06, 0.05), (0.06, 0.0), (0, -0.07), (-0.07, -0.12), (0.07, -0.14))}),
    (owl, "owl_snowy", {"tufts": False, "body": "#f4f1ea", "belly": "#ffffff", "wing": "#e6e1d6", "face": "#ffffff", "iris": "#f2c23a", "beak": "#3a3330", "talon": "#3a3330", "fleck": "#3a3330",
                        "flecks": ((-0.06, 0.05), (0.06, 0.0), (0, -0.07), (-0.07, -0.12), (0.07, -0.14))}),
]

only = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
for build, name, colours in ROSTER:
    if not only or any(name.startswith(prefix) for prefix in only):
        build(name, colours)
