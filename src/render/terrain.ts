import * as THREE from 'three/webgpu';
import { HEX_SCALE, tileRimStretch } from '../world/hex';
import { bridgeRotation, groundHeight, worldMap } from '../world/map';
import type { Assets } from './assets';
import type { SceneryBuilder } from './scenery';

const TILE_ROTATION_STEP = -Math.PI / 3;
const BRIDGE_MODEL = 'building_bridge_A';

/**
 * A copy of a hex tile whose flat top reaches all the way to its edges (see
 * tileRimStretch), so neighbouring tiles meet without a groove at the seam.
 * The template is left alone: its geometry may be shared with other models.
 */
function seamlessTile(template: THREE.Object3D): THREE.Object3D {
  const tile = template.clone();
  tile.updateMatrixWorld(true);
  const toTile = new THREE.Matrix4().copy(tile.matrixWorld).invert();
  const point = new THREE.Vector3();
  tile.traverse((object) => {
    const mesh = object as THREE.Mesh;
    if (!mesh.isMesh) return;
    // Stretch in the tile's own frame, in case a mesh is not at the model's origin.
    const local = new THREE.Matrix4().multiplyMatrices(toTile, mesh.matrixWorld);
    const back = local.clone().invert();
    const source = mesh.geometry.getAttribute('position');
    const positions = new Float32Array(source.count * 3);
    for (let i = 0; i < source.count; i++) {
      point.fromBufferAttribute(source, i).applyMatrix4(local);
      const stretch = tileRimStretch(point.x, point.z);
      point.set(point.x * stretch, point.y, point.z * stretch).applyMatrix4(back);
      point.toArray(positions, i * 3);
    }
    const geometry = mesh.geometry.clone();
    geometry.setAttribute('position', new THREE.BufferAttribute(positions, 3));
    geometry.computeBoundingBox();
    geometry.computeBoundingSphere();
    mesh.geometry = geometry;
  });
  return tile;
}

/** Adds the hex tiles, their decorations and the bridges to the scenery. */
export class Terrain {
  constructor(assets: Assets, builder: SceneryBuilder) {
    const bridge = assets.model('hex', BRIDGE_MODEL);
    const tiles = new Map<string, THREE.Object3D>();
    const tile = (model: string): THREE.Object3D => {
      let seamless = tiles.get(model);
      if (!seamless) {
        seamless = seamlessTile(assets.model('hex', model));
        tiles.set(model, seamless);
      }
      return seamless;
    };
    for (const cell of worldMap.cells) {
      builder.add(tile(cell.tile.model), {
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

  /** Ground height in world units at a world position (0 on plain grass). */
  heightAt(x: number, z: number): number {
    return groundHeight(x, z);
  }
}
