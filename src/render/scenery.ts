import * as THREE from 'three/webgpu';

/** World is cut into square chunks so off-screen scenery is frustum-culled together. */
const CHUNK_SIZE = 48;
/**
 * A model seen this often in one chunk is drawn as instances. Anything rarer
 * is baked into one mesh per material and chunk: an instanced draw for one or
 * two copies costs a whole draw call, and there are hundreds of such models.
 */
export const MIN_INSTANCES = 8;
/** The name prefix `Assets` gives the untextured, single-colour materials. */
const FLAT_PREFIX = 'flat:';

export interface Part {
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
  chunk: string;
}

interface Piece {
  geometry: THREE.BufferGeometry;
  matrix: THREE.Matrix4;
  /** paints the whole piece this colour; without it the geometry's own vertex colours are kept */
  color?: THREE.Color;
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
 * Copies of geometries, each moved by its matrix, as one indexed geometry with
 * positions, normals and, when every piece has them, UVs and vertex colours.
 */
function bake(pieces: readonly Piece[]): THREE.BufferGeometry {
  let vertices = 0;
  let indices = 0;
  let withUv = true;
  let withColor = true;
  for (const { geometry, color } of pieces) {
    const count = geometry.getAttribute('position').count;
    vertices += count;
    indices += geometry.index ? geometry.index.count : count;
    withUv &&= geometry.hasAttribute('uv');
    withColor &&= color !== undefined || geometry.hasAttribute('color');
  }
  const positions = new Float32Array(vertices * 3);
  const normals = new Float32Array(vertices * 3);
  const uvs = withUv ? new Float32Array(vertices * 2) : null;
  const colors = withColor ? new Float32Array(vertices * 3) : null;
  const index = vertices > 65535 ? new Uint32Array(indices) : new Uint16Array(indices);
  const point = new THREE.Vector3();
  const normalMatrix = new THREE.Matrix3();
  let v = 0;
  let i = 0;
  for (const { geometry, matrix, color } of pieces) {
    const position = geometry.getAttribute('position');
    const normal = geometry.getAttribute('normal');
    const uv = geometry.getAttribute('uv');
    const own = geometry.getAttribute('color');
    normalMatrix.getNormalMatrix(matrix);
    for (let k = 0; k < position.count; k++) {
      point.fromBufferAttribute(position, k).applyMatrix4(matrix).toArray(positions, (v + k) * 3);
      if (normal) point.fromBufferAttribute(normal, k).applyMatrix3(normalMatrix).normalize().toArray(normals, (v + k) * 3);
      if (uvs && uv) {
        uvs[(v + k) * 2] = uv.getX(k);
        uvs[(v + k) * 2 + 1] = uv.getY(k);
      }
      if (colors) {
        if (color) color.toArray(colors, (v + k) * 3);
        else if (own) point.fromBufferAttribute(own, k).toArray(colors, (v + k) * 3);
      }
    }
    if (geometry.index) {
      for (let k = 0; k < geometry.index.count; k++) index[i++] = v + geometry.index.getX(k);
    } else {
      for (let k = 0; k < position.count; k++) index[i++] = v + k;
    }
    v += position.count;
  }
  const baked = new THREE.BufferGeometry();
  baked.setAttribute('position', new THREE.BufferAttribute(positions, 3));
  baked.setAttribute('normal', new THREE.BufferAttribute(normals, 3));
  if (uvs) baked.setAttribute('uv', new THREE.BufferAttribute(uvs, 2));
  if (colors) baked.setAttribute('color', new THREE.BufferAttribute(colors, 3));
  baked.setIndex(new THREE.BufferAttribute(index, 1));
  return baked;
}

/**
 * Repacks a baked geometry into short integers, about half the memory: the
 * positions become fractions of the geometry's own extent, which the returned
 * matrix scales back, so the mesh that draws it must carry that matrix.
 */
function pack(geometry: THREE.BufferGeometry): THREE.Matrix4 {
  geometry.computeBoundingBox();
  const box = geometry.boundingBox as THREE.Box3;
  const center = box.getCenter(new THREE.Vector3());
  const size = box.getSize(new THREE.Vector3());
  // One scale for all three axes, so the normals stay true.
  const half = Math.max(size.x, size.y, size.z, 1e-6) / 2;

  const position = geometry.getAttribute('position');
  const packed = new Int16Array(position.count * 3);
  for (let k = 0; k < position.count; k++) {
    packed[k * 3] = Math.round(((position.getX(k) - center.x) / half) * 32767);
    packed[k * 3 + 1] = Math.round(((position.getY(k) - center.y) / half) * 32767);
    packed[k * 3 + 2] = Math.round(((position.getZ(k) - center.z) / half) * 32767);
  }
  geometry.setAttribute('position', new THREE.BufferAttribute(packed, 3, true));

  const normal = geometry.getAttribute('normal');
  const normals = new Int8Array(normal.count * 3);
  for (let k = 0; k < normals.length; k++) normals[k] = Math.round((normal.array[k] as number) * 127);
  geometry.setAttribute('normal', new THREE.BufferAttribute(normals, 3, true));

  for (const name of ['uv', 'color']) {
    const attribute = geometry.getAttribute(name);
    if (!attribute) continue;
    const source = attribute.array as Float32Array;
    if (source.some((value) => value < 0 || value > 1)) continue; // tiling UVs stay as floats
    const shorts = new Uint16Array(source.length);
    for (let k = 0; k < source.length; k++) shorts[k] = Math.round((source[k] as number) * 65535);
    geometry.setAttribute(name, new THREE.BufferAttribute(shorts, attribute.itemSize, true));
  }
  return new THREE.Matrix4().compose(center, new THREE.Quaternion(), new THREE.Vector3(half, half, half));
}

/**
 * A model's meshes joined into one part per material, remembered per template
 * so every layer that draws the model (the merged scenery, the trees) shares
 * the same geometries.
 */
export class PartCache {
  private readonly parts = new WeakMap<THREE.Object3D, Part[]>();
  /** Every single-colour material is drawn by this one, its colour moved into the vertices, so such parts can share a mesh. */
  private readonly painted = new THREE.MeshLambertMaterial({ vertexColors: true });

  /** A model's meshes, joined so that there is one part per material. */
  partsOf(template: THREE.Object3D): Part[] {
    let parts = this.parts.get(template);
    if (!parts) {
      template.updateMatrixWorld(true);
      const inverseRoot = new THREE.Matrix4().copy(template.matrixWorld).invert();
      const byMaterial = new Map<THREE.Material, Piece[]>();
      template.traverse((object) => {
        const mesh = object as THREE.Mesh;
        if (!mesh.isMesh) return;
        // Multi-material meshes are rare here; drawing them with the first material is fine.
        const own = (Array.isArray(mesh.material) ? mesh.material[0] : mesh.material) as THREE.Material;
        const flat = own.name.startsWith(FLAT_PREFIX);
        const material = flat ? this.painted : own;
        const piece: Piece = { geometry: mesh.geometry, matrix: new THREE.Matrix4().multiplyMatrices(inverseRoot, mesh.matrixWorld) };
        if (flat) piece.color = (own as THREE.MeshLambertMaterial).color;
        const pieces = byMaterial.get(material);
        if (pieces) pieces.push(piece);
        else byMaterial.set(material, [piece]);
      });
      parts = [];
      for (const [material, pieces] of byMaterial) {
        const only = pieces[0] as Piece;
        // A lone textured mesh is used as it is, so its geometry stays shared with every other user of the model.
        if (pieces.length === 1 && !only.color) parts.push({ geometry: only.geometry, material, local: only.matrix });
        else parts.push({ geometry: bake(pieces), material, local: new THREE.Matrix4() });
      }
      this.parts.set(template, parts);
    }
    return parts;
  }
}

/**
 * Collects static scenery and builds it with as few draw calls as it can: an
 * InstancedMesh per (geometry, material, chunk) for models that repeat, and
 * one baked mesh per (material, chunk) for the rest.
 */
export class SceneryBuilder {
  private readonly batches = new Map<string, Batch>();
  private readonly position = new THREE.Vector3();
  private readonly quaternion = new THREE.Quaternion();
  private readonly scale = new THREE.Vector3();
  private readonly up = new THREE.Vector3(0, 1, 0);
  count = 0;

  constructor(private readonly cache = new PartCache()) {}

  add(template: THREE.Object3D, options: PlaceOptions): void {
    const s = options.scale ?? 1;
    this.position.set(options.x, options.y ?? 0, options.z);
    this.quaternion.setFromAxisAngle(this.up, options.rotation ?? 0);
    this.scale.set(s, s, s);
    const world = new THREE.Matrix4().compose(this.position, this.quaternion, this.scale);
    const chunk = `${Math.floor(options.x / CHUNK_SIZE)},${Math.floor(options.z / CHUNK_SIZE)}`;
    const castShadow = options.castShadow ?? true;
    for (const part of this.cache.partsOf(template)) {
      const key = `${part.geometry.uuid}|${part.material.uuid}|${chunk}|${castShadow ? 1 : 0}`;
      let batch = this.batches.get(key);
      if (!batch) {
        batch = { geometry: part.geometry, material: part.material, matrices: [], castShadow, chunk };
        this.batches.set(key, batch);
      }
      batch.matrices.push(new THREE.Matrix4().multiplyMatrices(world, part.local));
    }
    this.count++;
  }

  build(): THREE.Group {
    const group = new THREE.Group();
    group.name = 'scenery';
    const rare = new Map<string, Batch[]>();
    for (const batch of this.batches.values()) {
      if (batch.matrices.length < MIN_INSTANCES) {
        const key = `${batch.material.uuid}|${batch.chunk}|${batch.castShadow ? 1 : 0}`;
        const list = rare.get(key);
        if (list) list.push(batch);
        else rare.set(key, [batch]);
        continue;
      }
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
    for (const batches of rare.values()) {
      const first = batches[0] as Batch;
      const geometry = bake(batches.flatMap((batch) => batch.matrices.map((matrix) => ({ geometry: batch.geometry, matrix }))));
      const mesh = new THREE.Mesh(geometry, first.material);
      mesh.matrix.copy(pack(geometry));
      mesh.matrixAutoUpdate = false;
      mesh.matrixWorldNeedsUpdate = true;
      geometry.computeBoundingSphere();
      geometry.computeBoundingBox();
      mesh.castShadow = first.castShadow;
      mesh.receiveShadow = true;
      group.add(mesh);
    }
    this.batches.clear();
    return group;
  }
}
