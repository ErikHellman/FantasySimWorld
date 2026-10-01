import * as THREE from 'three/webgpu';

/** World is cut into square chunks so off-screen scenery is frustum-culled together. */
const CHUNK_SIZE = 48;

interface Part {
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  /** mesh transform relative to the model root */
  local: THREE.Matrix4;
}

interface Batch {
  geometry: THREE.BufferGeometry;
  material: THREE.Material;
  matrices: THREE.Matrix4[];
  castShadow: boolean;
}

export interface PlaceOptions {
  x: number;
  y?: number;
  z: number;
  /** rotation about y in radians */
  rotation?: number;
  scale?: number;
  castShadow?: boolean;
}

/**
 * Collects static scenery and builds one InstancedMesh per (geometry, material, chunk).
 */
export class SceneryBuilder {
  private readonly parts = new WeakMap<THREE.Object3D, Part[]>();
  private readonly batches = new Map<string, Batch>();
  private readonly position = new THREE.Vector3();
  private readonly quaternion = new THREE.Quaternion();
  private readonly scale = new THREE.Vector3();
  private readonly up = new THREE.Vector3(0, 1, 0);
  count = 0;

  private partsOf(template: THREE.Object3D): Part[] {
    let parts = this.parts.get(template);
    if (!parts) {
      parts = [];
      template.updateMatrixWorld(true);
      const inverseRoot = new THREE.Matrix4().copy(template.matrixWorld).invert();
      template.traverse((object) => {
        const mesh = object as THREE.Mesh;
        if (!mesh.isMesh) return;
        const materials = Array.isArray(mesh.material) ? mesh.material : [mesh.material];
        const local = new THREE.Matrix4().multiplyMatrices(inverseRoot, mesh.matrixWorld);
        // Multi-material meshes are rare here; drawing them with the first material is fine.
        parts!.push({ geometry: mesh.geometry, material: materials[0] as THREE.Material, local });
      });
      this.parts.set(template, parts);
    }
    return parts;
  }

  add(template: THREE.Object3D, options: PlaceOptions): void {
    const s = options.scale ?? 1;
    this.position.set(options.x, options.y ?? 0, options.z);
    this.quaternion.setFromAxisAngle(this.up, options.rotation ?? 0);
    this.scale.set(s, s, s);
    const world = new THREE.Matrix4().compose(this.position, this.quaternion, this.scale);
    const chunk = `${Math.floor(options.x / CHUNK_SIZE)},${Math.floor(options.z / CHUNK_SIZE)}`;
    const castShadow = options.castShadow ?? true;
    for (const part of this.partsOf(template)) {
      const key = `${part.geometry.uuid}|${part.material.uuid}|${chunk}|${castShadow ? 1 : 0}`;
      let batch = this.batches.get(key);
      if (!batch) {
        batch = { geometry: part.geometry, material: part.material, matrices: [], castShadow };
        this.batches.set(key, batch);
      }
      batch.matrices.push(new THREE.Matrix4().multiplyMatrices(world, part.local));
    }
    this.count++;
  }

  build(): THREE.Group {
    const group = new THREE.Group();
    group.name = 'scenery';
    for (const batch of this.batches.values()) {
      const mesh = new THREE.InstancedMesh(batch.geometry, batch.material, batch.matrices.length);
      batch.matrices.forEach((matrix, index) => mesh.setMatrixAt(index, matrix));
      mesh.instanceMatrix.needsUpdate = true;
      mesh.castShadow = batch.castShadow;
      mesh.receiveShadow = true;
      mesh.computeBoundingSphere();
      mesh.computeBoundingBox();
      mesh.matrixAutoUpdate = false;
      group.add(mesh);
    }
    this.batches.clear();
    return group;
  }
}
