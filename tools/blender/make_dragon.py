"""Generates the dragon: a rigged, animated model in the style of the KayKit characters.

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/blender/make_dragon.py

Writes tools/blender/rigged/Dragon.glb (mesh, skeleton and every clip), which
`pnpm assets` turns into a character and a rig of its own. It follows the
conventions of make_props.py: metres-ish units with a villager about 2.4 tall,
origin at ground level in the middle, chunky bevelled shapes, flat colours. The
body is made of rigid parts, each bound wholly to one bone, as the KayKit
characters are. It faces glTF +z (Blender -y), like every other character.
"""
import math
import os

import bpy
from mathutils import Euler, Matrix, Vector

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "rigged")
os.makedirs(OUT, exist_ok=True)
FPS = 30

# Colours picked to sit beside the KayKit palette.
PALETTE = {
    "scale": (0.72, 0.15, 0.10),
    "scale_dark": (0.46, 0.08, 0.06),
    "belly": (0.96, 0.78, 0.46),
    "membrane": (0.92, 0.42, 0.16),
    "horn": (0.26, 0.20, 0.17),
    "claw": (0.94, 0.90, 0.80),
    "mouth": (0.30, 0.04, 0.04),
    "eye": (1.0, 0.72, 0.10),
}
GLOWING = {"eye": 4.0}

_materials = {}


def material(name):
    if name not in _materials:
        mat = bpy.data.materials.new("custom_dragon_" + name)
        mat.use_nodes = True
        bsdf = mat.node_tree.nodes.get("Principled BSDF")
        r, g, b = PALETTE[name]
        bsdf.inputs["Base Color"].default_value = (r, g, b, 1.0)
        bsdf.inputs["Roughness"].default_value = 0.6
        bsdf.inputs["Metallic"].default_value = 0.0
        if name in GLOWING:
            bsdf.inputs["Emission Color"].default_value = (r, g, b, 1.0)
            bsdf.inputs["Emission Strength"].default_value = GLOWING[name]
        _materials[name] = mat
    return _materials[name]


def reset():
    for obj in list(bpy.data.objects):
        bpy.data.objects.remove(obj, do_unlink=True)
    for action in list(bpy.data.actions):
        bpy.data.actions.remove(action)


# ---------------------------------------------------------------- skeleton

# Bone name -> (head, tail, parent). Blender is z-up and the dragon looks along -y, so its left is +x.
BONES = {
    "root": ((0, 0, 0), (0, 0, 0.6), None),
    "body": ((0, 0.1, 2.05), (0, -1.1, 2.1), "root"),
    "neck_1": ((0, -1.25, 2.45), (0, -1.75, 3.05), "body"),
    "neck_2": ((0, -1.75, 3.05), (0, -2.25, 3.6), "neck_1"),
    "head": ((0, -2.25, 3.6), (0, -3.35, 3.7), "neck_2"),
    "jaw": ((0, -2.45, 3.45), (0, -3.35, 3.38), "head"),
    "tail_1": ((0, 1.25, 2.1), (0, 2.25, 1.95), "body"),
    "tail_2": ((0, 2.25, 1.95), (0, 3.15, 1.75), "tail_1"),
    "tail_3": ((0, 3.15, 1.75), (0, 3.95, 1.6), "tail_2"),
    "tail_4": ((0, 3.95, 1.6), (0, 4.65, 1.5), "tail_3"),
}
for side, sx in (("l", 1), ("r", -1)):
    BONES.update({
        f"wing_upper_{side}": ((sx * 0.7, -0.7, 2.65), (sx * 2.6, -0.5, 2.95), "body"),
        f"wing_lower_{side}": ((sx * 2.6, -0.5, 2.95), (sx * 4.7, 0.05, 3.05), f"wing_upper_{side}"),
        f"arm_upper_{side}": ((sx * 0.62, -0.95, 1.65), (sx * 0.7, -1.05, 0.85), "body"),
        f"arm_lower_{side}": ((sx * 0.7, -1.05, 0.85), (sx * 0.7, -1.15, 0.12), f"arm_upper_{side}"),
        f"leg_upper_{side}": ((sx * 0.68, 0.75, 1.7), (sx * 0.75, 1.05, 0.9), "body"),
        f"leg_lower_{side}": ((sx * 0.75, 1.05, 0.9), (sx * 0.75, 0.8, 0.12), f"leg_upper_{side}"),
    })


def make_armature():
    data = bpy.data.armatures.new("Rig_Dragon")
    rig = bpy.data.objects.new("Rig_Dragon", data)
    bpy.context.scene.collection.objects.link(rig)
    bpy.context.view_layer.objects.active = rig
    rig.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    for name, (head, tail, parent) in BONES.items():
        bone = data.edit_bones.new(name)
        bone.head = head
        bone.tail = tail
        bone.roll = 0
        if parent:
            bone.parent = data.edit_bones[parent]
    bpy.ops.object.mode_set(mode="OBJECT")
    return rig


# ---------------------------------------------------------------- body parts

parts = []


def finish(obj, colour, bone, bevel):
    obj.data.materials.append(material(colour))
    if bevel > 0:
        modifier = obj.modifiers.new("bevel", "BEVEL")
        modifier.width = bevel
        # Big parts get a rounder edge, as the KayKit characters have.
        modifier.segments = 2 if bevel >= 0.1 else 1
        modifier.limit_method = "ANGLE"
    for polygon in obj.data.polygons:
        polygon.use_smooth = False
    obj["bone"] = bone
    parts.append(obj)
    return obj


def box(size, location, colour, bone, rotation=(0, 0, 0), bevel=None):
    """A box, its edges rounded in proportion to its smallest side unless `bevel` says otherwise."""
    bpy.ops.mesh.primitive_cube_add(size=1, location=location, rotation=rotation)
    obj = bpy.context.active_object
    obj.scale = size
    bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
    return finish(obj, colour, bone, min(size) * 0.22 if bevel is None else bevel)


def cone(radius, depth, location, colour, bone, rotation=(0, 0, 0), vertices=6, top=0.0):
    bpy.ops.mesh.primitive_cone_add(vertices=vertices, radius1=radius, radius2=radius * top, depth=depth, location=location, rotation=rotation)
    return finish(bpy.context.active_object, colour, bone, 0)


def towards(start, end):
    """Rotation that turns a primitive's +z onto the line from start to end."""
    return (Vector(end) - Vector(start)).to_track_quat("Z", "Y").to_euler()


def spike(base, tip, radius, colour, bone, vertices=5):
    base, tip = Vector(base), Vector(tip)
    return cone(radius, (tip - base).length, (base + tip) / 2, colour, bone, rotation=towards(base, tip), vertices=vertices)


def limb(start, end, width, height, colour, bone, taper=1.0, bevel=None):
    """A box from start to end, `width` across and `height` deep, narrowing to `taper` at the end."""
    start, end = Vector(start), Vector(end)
    direction = end - start
    rotation = direction.to_track_quat("Y", "Z").to_euler()
    obj = box((width, direction.length, height), (start + end) / 2, colour, bone, rotation=rotation, bevel=bevel)
    if taper != 1.0:
        for vertex in obj.data.vertices:
            if vertex.co.y > 0:
                vertex.co.x *= taper
                vertex.co.z *= taper
    return obj


def panel(points, colour, bone, thickness=0.08):
    """A flat membrane through the given outline, made solid."""
    mesh = bpy.data.meshes.new("panel")
    mesh.from_pydata([Vector(p) for p in points], [], [list(range(len(points)))])
    obj = bpy.data.objects.new("panel", mesh)
    bpy.context.scene.collection.objects.link(obj)
    solid = obj.modifiers.new("solid", "SOLIDIFY")
    solid.thickness = thickness
    solid.offset = 0
    return finish(obj, colour, bone, 0)


def build_body():
    # The torso: a broad, rounded barrel with a deeper chest, a cream belly and a ridge of spikes.
    box((1.9, 2.5, 1.6), (0, 0.15, 2.05), "scale", "body", bevel=0.4)
    box((1.75, 1.2, 1.5), (0, -0.95, 2.22), "scale", "body", rotation=(math.radians(-12), 0, 0), bevel=0.38)
    box((1.3, 2.5, 0.24), (0, -0.2, 1.3), "belly", "body", rotation=(math.radians(-4), 0, 0), bevel=0.1)
    for index, y in enumerate((-1.15, -0.45, 0.25, 0.95)):
        spike((0, y, 2.75), (0, y + 0.4, 3.45 - index * 0.08), 0.24, "horn", "body")

    # The neck, in two segments, with belly plates down the front.
    limb((0, -1.2, 2.4), (0, -1.85, 3.15), 1.05, 1.0, "scale", "neck_1", taper=0.9)
    limb((0, -1.7, 3.0), (0, -2.35, 3.7), 0.95, 0.9, "scale", "neck_2", taper=0.9)
    limb((0, -1.55, 2.15), (0, -2.05, 2.8), 0.72, 0.2, "belly", "neck_1", bevel=0.06)
    limb((0, -2.05, 2.85), (0, -2.55, 3.38), 0.64, 0.2, "belly", "neck_2", bevel=0.06)
    spike((0, -1.6, 3.15), (0, -1.3, 3.65), 0.18, "horn", "neck_1")

    # The head: big and round-edged like a KayKit face, with glowing eyes under heavy brows,
    # horns swept back, and a jaw on its own bone.
    box((1.25, 1.2, 1.0), (0, -2.7, 3.92), "scale", "head", bevel=0.28)
    box((0.95, 0.95, 0.6), (0, -3.45, 3.78), "scale", "head", bevel=0.2)
    for sx in (1, -1):
        box((0.16, 0.14, 0.1), (sx * 0.2, -3.92, 3.98), "scale_dark", "head", bevel=0.03)  # nostrils
        box((0.12, 0.34, 0.28), (sx * 0.6, -2.98, 4.02), "eye", "head", bevel=0.04)
        box((0.36, 0.55, 0.16), (sx * 0.45, -2.95, 4.36), "scale_dark", "head", rotation=(0, sx * math.radians(-12), 0), bevel=0.06)
        spike((sx * 0.35, -2.45, 4.3), (sx * 0.62, -1.85, 4.85), 0.2, "horn", "head", vertices=6)
        spike((sx * 0.62, -1.85, 4.85), (sx * 0.7, -1.45, 5.0), 0.1, "horn", "head", vertices=6)
        spike((sx * 0.62, -2.5, 3.95), (sx * 1.05, -2.2, 4.15), 0.1, "horn", "head")
        for y in (-3.75, -3.4):
            spike((sx * 0.32, y, 3.52), (sx * 0.32, y, 3.34), 0.06, "claw", "head", vertices=4)
    box((0.85, 1.05, 0.3), (0, -3.2, 3.38), "scale", "jaw", bevel=0.1)
    box((0.62, 0.85, 0.06), (0, -3.2, 3.54), "mouth", "jaw", bevel=0.0)
    for sx in (1, -1):
        for y in (-3.6, -3.25):
            spike((sx * 0.3, y, 3.5), (sx * 0.3, y, 3.68), 0.055, "claw", "jaw", vertices=4)

    # The wings: a bony arm with a claw at the elbow, two spars and two panels of membrane.
    for side, sx in (("l", 1), ("r", -1)):
        limb((sx * 0.7, -0.7, 2.65), (sx * 2.65, -0.5, 2.95), 0.36, 0.36, "scale_dark", f"wing_upper_{side}", taper=0.8)
        limb((sx * 2.6, -0.5, 2.95), (sx * 4.75, 0.05, 3.05), 0.26, 0.26, "scale_dark", f"wing_lower_{side}", taper=0.55)
        spike((sx * 2.6, -0.55, 3.05), (sx * 2.75, -1.0, 3.2), 0.1, "claw", f"wing_lower_{side}")
        panel([(sx * 0.75, -0.6, 2.62), (sx * 2.6, -0.45, 2.92), (sx * 2.55, 1.15, 2.72), (sx * 1.6, 0.9, 2.6), (sx * 0.75, 1.1, 2.45)], "membrane", f"wing_upper_{side}")
        panel([(sx * 2.6, -0.45, 2.92), (sx * 4.7, 0.05, 3.03), (sx * 4.1, 1.35, 2.85), (sx * 3.45, 1.0, 2.8), (sx * 3.0, 1.6, 2.76), (sx * 2.55, 1.15, 2.72)], "membrane", f"wing_lower_{side}")
        limb((sx * 2.65, -0.45, 2.92), (sx * 4.1, 1.35, 2.85), 0.12, 0.12, "scale_dark", f"wing_lower_{side}", bevel=0.03)
        limb((sx * 2.62, -0.45, 2.92), (sx * 3.0, 1.6, 2.76), 0.12, 0.12, "scale_dark", f"wing_lower_{side}", bevel=0.03)

        # Four stout legs: heavy thighs behind, arms in front, each with a clawed foot.
        limb((sx * 0.62, -0.95, 1.75), (sx * 0.7, -1.05, 0.85), 0.52, 0.56, "scale", f"arm_upper_{side}", taper=0.85)
        limb((sx * 0.7, -1.05, 0.9), (sx * 0.7, -1.15, 0.2), 0.42, 0.44, "scale", f"arm_lower_{side}", taper=0.9)
        limb((sx * 0.68, 0.75, 1.9), (sx * 0.75, 1.05, 0.9), 0.66, 0.85, "scale", f"leg_upper_{side}", taper=0.8)
        limb((sx * 0.75, 1.05, 0.95), (sx * 0.75, 0.8, 0.2), 0.46, 0.48, "scale", f"leg_lower_{side}", taper=0.9)
        for bone, (x, y) in ((f"arm_lower_{side}", (sx * 0.7, -1.2)), (f"leg_lower_{side}", (sx * 0.75, 0.72))):
            box((0.56, 0.66, 0.26), (x, y - 0.08, 0.13), "scale_dark", bone, bevel=0.08)
            for dx in (-0.17, 0.0, 0.17):
                spike((x + dx, y - 0.38, 0.12), (x + dx, y - 0.65, 0.04), 0.07, "claw", bone, vertices=4)

    # The tail: tapering segments, a spike on each, and a spade at the tip.
    sizes = ((1.05, 0.95), (0.82, 0.74), (0.6, 0.56), (0.42, 0.4))
    for index, (width, height) in enumerate(sizes):
        name = f"tail_{index + 1}"
        head, tail, _parent = BONES[name]
        limb(head, tail, width, height, "scale", name, taper=0.85)
        mid = (Vector(head) + Vector(tail)) / 2
        spike((0, mid.y, mid.z + height * 0.4), (0, mid.y + 0.3, mid.z + height * 0.4 + 0.45 - index * 0.07), 0.16 - index * 0.025, "horn", name)
    panel([(0, 4.45, 1.55), (0.5, 4.9, 1.5), (0, 5.5, 1.42), (-0.5, 4.9, 1.5)], "horn", "tail_4", thickness=0.14)


def bind(rig):
    """Joins the parts into one mesh, each part wholly weighted to its bone."""
    for obj in parts:
        group = obj.vertex_groups.new(name=obj["bone"])
        group.add(list(range(len(obj.data.vertices))), 1.0, "REPLACE")
    bpy.ops.object.select_all(action="DESELECT")
    for obj in parts:
        obj.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.convert(target="MESH")
    bpy.ops.object.join()
    body = bpy.context.active_object
    body.name = "Dragon"
    body.data.name = "Dragon"
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    body.parent = rig
    modifier = body.modifiers.new("rig", "ARMATURE")
    modifier.object = rig
    return body


# ---------------------------------------------------------------- animation

def rest(rig, name):
    return rig.data.bones[name].matrix_local.to_3x3()


def key(rig, frame, pose, lift=0.0, sink=None):
    """Keys every bone at `frame`. `pose` maps a bone to a rotation in degrees about the
    armature's axes, as if its parent were at rest; `lift` raises the body."""
    for name, bone in rig.pose.bones.items():
        degrees = pose.get(name, (0, 0, 0))
        turn = Euler([math.radians(a) for a in degrees], "XYZ").to_matrix()
        r = rest(rig, name)
        bone.rotation_mode = "QUATERNION"
        bone.rotation_quaternion = (r.inverted() @ turn @ r).to_quaternion()
        bone.keyframe_insert("rotation_quaternion", frame=frame)
        if name == "body":
            offset = Vector((0, 0, lift)) if sink is None else Vector(sink)
            bone.location = r.inverted() @ offset
            bone.keyframe_insert("location", frame=frame)


def wings(up, fold=0.0, sweep=0.0):
    """Both wings raised `up` degrees at the shoulder, the outer half bent by `fold`, swept back by `sweep`."""
    return {
        "wing_upper_l": (0, -up, -sweep),
        "wing_upper_r": (0, up, sweep),
        "wing_lower_l": (0, -fold, -sweep * 0.5),
        "wing_lower_r": (0, fold, sweep * 0.5),
    }


def tucked(back=70):
    """Legs folded back under the body for flight."""
    return {
        "arm_upper_l": (-back * 0.4, 0, 0), "arm_upper_r": (-back * 0.4, 0, 0),
        "arm_lower_l": (back * 1.4, 0, 0), "arm_lower_r": (back * 1.4, 0, 0),
        "leg_upper_l": (back, 0, 0), "leg_upper_r": (back, 0, 0),
        "leg_lower_l": (20, 0, 0), "leg_lower_r": (20, 0, 0),
    }


def tail(sway, droop=0.0):
    return {f"tail_{i}": (droop, 0, sway * (0.6 + i * 0.25)) for i in range(1, 5)}


def merge(*poses):
    out = {}
    for pose in poses:
        out.update(pose)
    return out


def neck(pitch, head=0.0, jaw=0.0):
    """Positive pitch bends the neck down and forward; negative lifts it."""
    return {"neck_1": (pitch, 0, 0), "neck_2": (pitch * 0.8, 0, 0), "head": (head, 0, 0), "jaw": (jaw, 0, 0)}


def clip(rig, name, seconds, keys):
    """One action: `keys` is a list of (time in seconds, pose, body offset)."""
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    rig.animation_data_create()
    rig.animation_data.action = action
    for time, pose, offset in keys:
        frame = round(time * FPS)
        if isinstance(offset, tuple):
            key(rig, frame, pose, sink=offset)
        else:
            key(rig, frame, pose, lift=offset)
    assert abs(keys[-1][0] - seconds) < 1e-6, name
    return action


def animate(rig):
    flight = merge(tucked(), neck(-8, head=8))
    # A full beat: wings high, a strong downstroke, then back up. The body rises on the downstroke.
    clip(rig, "Dragon_Fly", 1.0, [
        (0.0, merge(flight, wings(40, fold=-15), tail(6)), 0.0),
        (0.3, merge(flight, wings(-28, fold=12), tail(0)), 0.25),
        (0.5, merge(flight, wings(-35, fold=20), tail(-6)), 0.2),
        (0.8, merge(flight, wings(10, fold=-30, sweep=10), tail(0)), -0.05),
        (1.0, merge(flight, wings(40, fold=-15), tail(6)), 0.0),
    ])

    # Hanging in the air: the body reared up, legs dangling, slow deep beats.
    upright = merge({"body": (-28, 0, 0)}, tucked(back=10), neck(25, head=10), tail(0, droop=18))
    clip(rig, "Dragon_Hover", 1.4, [
        (0.0, merge(upright, wings(45, fold=-20, sweep=-10), tail(8, droop=18)), 0.0),
        (0.5, merge(upright, wings(-30, fold=15, sweep=10), tail(0, droop=18)), 0.3),
        (0.8, merge(upright, wings(-38, fold=25, sweep=12), tail(-8, droop=18)), 0.25),
        (1.1, merge(upright, wings(15, fold=-35, sweep=-5), tail(0, droop=18)), 0.0),
        (1.4, merge(upright, wings(45, fold=-20, sweep=-10), tail(8, droop=18)), 0.0),
    ])

    # Fire: rear back with the jaw shut, then thrust the head down and forward with it wide open.
    clip(rig, "Dragon_Breath", 1.6, [
        (0.0, merge(upright, wings(20, fold=-10), tail(0, droop=18)), 0.0),
        (0.45, merge(upright, {"body": (-38, 0, 0)}, neck(-20, head=-15, jaw=5), wings(45, fold=-25, sweep=-10), tail(6, droop=24)), 0.2),
        (0.7, merge(upright, {"body": (-12, 0, 0)}, neck(45, head=20, jaw=38), wings(-20, fold=15, sweep=10), tail(-4, droop=10)), 0.0),
        (1.25, merge(upright, {"body": (-14, 0, 0)}, neck(42, head=18, jaw=36), wings(-10, fold=5, sweep=5), tail(4, droop=10)), 0.05),
        (1.6, merge(upright, wings(20, fold=-10), tail(0, droop=18)), 0.0),
    ])

    # A roar: head thrown up, jaw wide, wings spread to their fullest.
    clip(rig, "Dragon_Roar", 2.0, [
        (0.0, merge(upright, wings(20, fold=-10), tail(0, droop=18)), 0.0),
        (0.5, merge(upright, {"body": (-36, 0, 0)}, neck(-35, head=-20, jaw=40), wings(55, fold=-5, sweep=-15), tail(10, droop=20)), 0.25),
        (1.5, merge(upright, {"body": (-36, 0, 0)}, neck(-38, head=-24, jaw=42), wings(50, fold=-8, sweep=-15), tail(-10, droop=20)), 0.3),
        (2.0, merge(upright, wings(20, fold=-10), tail(0, droop=18)), 0.0),
    ])

    # Struck: a jolt back and to one side.
    clip(rig, "Dragon_Hit", 0.6, [
        (0.0, merge(upright, wings(20, fold=-10)), 0.0),
        (0.15, merge(upright, {"body": (-40, 8, 10)}, neck(-25, head=-20, jaw=25), wings(35, fold=-30, sweep=-15), tail(14, droop=25)), 0.15),
        (0.6, merge(upright, wings(20, fold=-10)), 0.0),
    ])

    # Death: wings crumple, the body rolls onto its side and drops to the ground, the neck lies flat.
    fallen = merge(
        {"body": (0, 80, 0)},
        neck(30, head=10, jaw=20),
        {"wing_upper_l": (0, 20, -60), "wing_lower_l": (0, 70, -40), "wing_upper_r": (0, -40, 30), "wing_lower_r": (0, -90, 20)},
        tucked(back=40),
        tail(25, droop=-10),
    )
    clip(rig, "Dragon_Death", 2.4, [
        (0.0, merge(upright, wings(20, fold=-10)), 0.0),
        (0.4, merge(upright, {"body": (-45, 20, 0)}, neck(-30, head=-25, jaw=40), wings(60, fold=10, sweep=-20)), 0.2),
        (1.4, merge(fallen, {"body": (0, 60, 0)}), (0, 0, -0.6)),
        (1.8, fallen, (0, 0, -1.05)),
        (2.4, fallen, (0, 0, -1.1)),
    ])
    rig.animation_data.action = None
    for bone in rig.pose.bones:
        bone.rotation_quaternion = (1, 0, 0, 0)
        bone.location = (0, 0, 0)


def export(rig, body):
    bpy.ops.object.select_all(action="DESELECT")
    rig.select_set(True)
    body.select_set(True)
    bpy.ops.export_scene.gltf(
        filepath=os.path.join(OUT, "Dragon.glb"),
        export_format="GLB",
        use_selection=True,
        export_yup=True,
        export_apply=True,
        export_skins=True,
        export_animations=True,
        export_animation_mode="ACTIONS",
        export_force_sampling=True,
        export_frame_step=1,
    )
    print("wrote Dragon")


reset()
bpy.context.scene.render.fps = FPS
rig = make_armature()
build_body()
body = bind(rig)
animate(rig)
export(rig, body)
