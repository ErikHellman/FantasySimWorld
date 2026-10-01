import { HEX_SCALE } from '../world/hex';
import { bridgeRotation, groundHeight, worldMap } from '../world/map';
import type { Assets } from './assets';
import type { SceneryBuilder } from './scenery';

const TILE_ROTATION_STEP = -Math.PI / 3;
const BRIDGE_MODEL = 'building_bridge_A';

/** Adds the hex tiles, their decorations and the bridges to the scenery. */
export class Terrain {
  constructor(assets: Assets, builder: SceneryBuilder) {
    const bridge = assets.model('hex', BRIDGE_MODEL);
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

  /** Ground height in world units at a world position (0 on plain grass). */
  heightAt(x: number, z: number): number {
    return groundHeight(x, z);
  }
}
