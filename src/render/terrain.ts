import * as THREE from 'three/webgpu';
import { DIR_COUNT, HEX_SCALE, dirVector, maskHas } from '../world/hex';
import { cellAtPoint, worldMap, type HexCell } from '../world/map';
import type { Assets } from './assets';
import type { SceneryBuilder } from './scenery';

const TILE_ROTATION_STEP = -Math.PI / 3;
const BRIDGE_MODEL = 'building_bridge_A';

/** Top-surface heights of a model sampled on a grid in model space. */
class HeightField {
  private readonly data: Float32Array;

  constructor(
    template: THREE.Object3D,
    private readonly minX: number,
    private readonly minZ: number,
    private readonly maxX: number,
    private readonly maxZ: number,
    private readonly nx: number,
    private readonly nz: number,
  ) {
    this.data = new Float32Array(nx * nz).fill(Number.NEGATIVE_INFINITY);
    const a = new THREE.Vector3();
    const b = new THREE.Vector3();
    const c = new THREE.Vector3();
    template.updateMatrixWorld(true);
    const inverseRoot = new THREE.Matrix4().copy(template.matrixWorld).invert();
    const local = new THREE.Matrix4();
    template.traverse((object) => {
      const mesh = object as THREE.Mesh;
      if (!mesh.isMesh) return;
      local.multiplyMatrices(inverseRoot, mesh.matrixWorld);
      const position = mesh.geometry.getAttribute('position');
      const index = mesh.geometry.getIndex();
      const triangles = (index ? index.count : position.count) / 3;
      for (let t = 0; t < triangles; t++) {
        const i0 = index ? index.getX(t * 3) : t * 3;
        const i1 = index ? index.getX(t * 3 + 1) : t * 3 + 1;
        const i2 = index ? index.getX(t * 3 + 2) : t * 3 + 2;
        a.fromBufferAttribute(position, i0).applyMatrix4(local);
        b.fromBufferAttribute(position, i1).applyMatrix4(local);
        c.fromBufferAttribute(position, i2).applyMatrix4(local);
        this.rasterize(a, b, c);
      }
    });
  }

  private rasterize(a: THREE.Vector3, b: THREE.Vector3, c: THREE.Vector3): void {
    const det = (b.z - c.z) * (a.x - c.x) + (c.x - b.x) * (a.z - c.z);
    if (Math.abs(det) < 1e-10) return; // vertical face
    const cellX = (this.maxX - this.minX) / this.nx;
    const cellZ = (this.maxZ - this.minZ) / this.nz;
    const x0 = Math.max(0, Math.floor((Math.min(a.x, b.x, c.x) - this.minX) / cellX));
    const x1 = Math.min(this.nx - 1, Math.floor((Math.max(a.x, b.x, c.x) - this.minX) / cellX));
    const z0 = Math.max(0, Math.floor((Math.min(a.z, b.z, c.z) - this.minZ) / cellZ));
    const z1 = Math.min(this.nz - 1, Math.floor((Math.max(a.z, b.z, c.z) - this.minZ) / cellZ));
    for (let iz = z0; iz <= z1; iz++) {
      const z = this.minZ + (iz + 0.5) * cellZ;
      for (let ix = x0; ix <= x1; ix++) {
        const x = this.minX + (ix + 0.5) * cellX;
        const l1 = ((b.z - c.z) * (x - c.x) + (c.x - b.x) * (z - c.z)) / det;
        const l2 = ((c.z - a.z) * (x - c.x) + (a.x - c.x) * (z - c.z)) / det;
        const l3 = 1 - l1 - l2;
        if (l1 < -0.01 || l2 < -0.01 || l3 < -0.01) continue;
        const y = l1 * a.y + l2 * b.y + l3 * c.y;
        const i = iz * this.nx + ix;
        if (y > (this.data[i] as number)) this.data[i] = y;
      }
    }
  }

  /** Height at a model-space point, or null outside the sampled surface. */
  sample(x: number, z: number): number | null {
    const fx = ((x - this.minX) / (this.maxX - this.minX)) * this.nx - 0.5;
    const fz = ((z - this.minZ) / (this.maxZ - this.minZ)) * this.nz - 0.5;
    const ix = Math.min(this.nx - 1, Math.max(0, Math.round(fx)));
    const iz = Math.min(this.nz - 1, Math.max(0, Math.round(fz)));
    const y = this.data[iz * this.nx + ix] as number;
    return Number.isFinite(y) ? y : null;
  }
}

function bridgeRotation(cell: HexCell): number {
  for (let dir = 0; dir < DIR_COUNT; dir++) {
    if (maskHas(cell.roadMask, dir)) {
      const v = dirVector(dir);
      return Math.atan2(v.x, v.z); // the bridge model runs along its local z
    }
  }
  return 0;
}

/** Hex tiles, their decorations and bridges; also answers "how high is the ground here". */
export class Terrain {
  private readonly tileHeights = new Map<string, HeightField>();
  private readonly bridgeHeights: HeightField;

  constructor(private readonly assets: Assets, builder: SceneryBuilder) {
    const bridge = assets.model('hex', BRIDGE_MODEL);
    this.bridgeHeights = new HeightField(bridge, -0.7, -1, 0.7, 1, 28, 48);

    for (const cell of worldMap.cells) {
      builder.add(assets.model('hex', cell.tile.model), {
        x: cell.center.x,
        z: cell.center.z,
        rotation: cell.tile.rot * TILE_ROTATION_STEP,
        scale: HEX_SCALE,
        castShadow: false,
      });
      if (cell.decoration) {
        builder.add(assets.model('hex', cell.decoration.model), {
          x: cell.center.x,
          z: cell.center.z,
          rotation: cell.decoration.rot * TILE_ROTATION_STEP,
          scale: HEX_SCALE,
        });
      }
      if (cell.bridge) {
        builder.add(bridge, { x: cell.center.x, z: cell.center.z, rotation: bridgeRotation(cell), scale: HEX_SCALE });
      }
    }
  }

  private heightsFor(model: string): HeightField {
    let field = this.tileHeights.get(model);
    if (!field) {
      field = new HeightField(this.assets.model('hex', model), -1, -1.16, 1, 1.16, 40, 46);
      this.tileHeights.set(model, field);
    }
    return field;
  }

  /** Ground height in world units at a world position (0 on plain grass). */
  heightAt(x: number, z: number): number {
    const cell = cellAtPoint(x, z);
    if (!cell) return 0;
    const dx = (x - cell.center.x) / HEX_SCALE;
    const dz = (z - cell.center.z) / HEX_SCALE;
    let height = sampleRotated(this.heightsFor(cell.tile.model), dx, dz, cell.tile.rot * TILE_ROTATION_STEP) ?? 0;
    if (cell.bridge) {
      const deck = sampleRotated(this.bridgeHeights, dx, dz, bridgeRotation(cell));
      if (deck !== null && Math.abs(rotatedX(dx, dz, bridgeRotation(cell))) < 0.45) height = Math.max(height, deck);
    }
    return height * HEX_SCALE;
  }
}

/** x of a world-frame offset expressed in the frame of a model rotated by `rotation` about y. */
function rotatedX(dx: number, dz: number, rotation: number): number {
  return dx * Math.cos(rotation) - dz * Math.sin(rotation);
}

function sampleRotated(field: HeightField, dx: number, dz: number, rotation: number): number | null {
  const cos = Math.cos(rotation);
  const sin = Math.sin(rotation);
  return field.sample(dx * cos - dz * sin, dx * sin + dz * cos);
}
