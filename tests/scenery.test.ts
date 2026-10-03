import * as THREE from 'three/webgpu';
import { describe, expect, it } from 'vitest';
import { MIN_INSTANCES, SceneryBuilder } from '../src/render/scenery';

const textured = new THREE.MeshLambertMaterial();
textured.name = 'pack';

function model(material: THREE.Material | THREE.Material[], size = 1): THREE.Object3D {
  const root = new THREE.Group();
  for (const [index, one] of (Array.isArray(material) ? material : [material]).entries()) {
    const mesh = new THREE.Mesh(new THREE.BoxGeometry(size, size, size), one);
    mesh.position.y = index;
    root.add(mesh);
  }
  return root;
}

function flat(hex: number): THREE.Material {
  const material = new THREE.MeshLambertMaterial({ color: hex });
  material.name = `flat:${material.color.getHexString()}`;
  return material;
}

describe('SceneryBuilder', () => {
  it('instances a model that repeats within a chunk', () => {
    const builder = new SceneryBuilder();
    const tree = model(textured);
    for (let i = 0; i < MIN_INSTANCES; i++) builder.add(tree, { x: i, z: 1 });
    const [mesh] = builder.build().children as THREE.InstancedMesh[];
    expect(mesh?.isInstancedMesh).toBe(true);
    expect(mesh?.count).toBe(MIN_INSTANCES);
  });

  it('bakes the rare models of a chunk into one mesh per material', () => {
    const builder = new SceneryBuilder();
    builder.add(model(textured), { x: 1, z: 1 });
    builder.add(model(textured, 2), { x: 5, z: 1 });
    builder.add(model(textured, 3), { x: 9, z: 1, castShadow: false });
    builder.add(model(textured), { x: 100, z: 1 });
    const meshes = builder.build().children as THREE.Mesh[];
    // Two in the first chunk (with and without a shadow) and one in the chunk further east.
    expect(meshes).toHaveLength(3);
    expect(meshes.every((mesh) => !(mesh as unknown as THREE.InstancedMesh).isInstancedMesh)).toBe(true);
    expect(meshes.map((mesh) => mesh.geometry.getAttribute('position').count).sort()).toEqual([24, 24, 48]);
  });

  it('keeps baked meshes where their models were placed', () => {
    const builder = new SceneryBuilder();
    builder.add(model(textured, 2), { x: 10, y: 3, z: 20 });
    const [mesh] = builder.build().children as THREE.Mesh[];
    mesh!.updateMatrixWorld(true);
    const box = new THREE.Box3().setFromObject(mesh!);
    expect(box.min.x).toBeCloseTo(9, 2);
    expect(box.max.x).toBeCloseTo(11, 2);
    expect(box.min.y).toBeCloseTo(2, 2);
    expect(box.max.z).toBeCloseTo(21, 2);
  });

  it('draws single-colour parts with one shared material, the colours in the vertices', () => {
    const builder = new SceneryBuilder();
    builder.add(model([flat(0xff0000), flat(0x00ff00)]), { x: 1, z: 1 });
    builder.add(model(flat(0x0000ff)), { x: 3, z: 1 });
    const meshes = builder.build().children as THREE.Mesh[];
    expect(meshes).toHaveLength(1);
    const color = meshes[0]!.geometry.getAttribute('color');
    expect(color.count).toBe(72);
    expect([color.getX(0), color.getY(0), color.getZ(0)]).toEqual([1, 0, 0]);
    expect((meshes[0]!.material as THREE.MeshLambertMaterial).vertexColors).toBe(true);
  });
});
