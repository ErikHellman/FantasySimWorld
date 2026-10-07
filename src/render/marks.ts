import * as THREE from 'three/webgpu';
import type { WorldObject } from '../generated/api';
import { hashString } from '../core/rng';
import { hexAt } from '../world/hex';
import { cellAt } from '../world/map';
import { places, type PlacedModel } from '../world/places';
import { scatter } from '../world/scatter';
import type { Assets } from './assets';
import type { QualitySettings } from './quality';
import type { SceneryBuilder, SceneryHandle } from './scenery';

/** What lies where a prop was taken apart: a couple of planks and a log, flat on the ground. */
const PIECES: [string, string, number, number, number][] = [
  ['resource', 'Wood_Plank_A', 0.5, 0.3, 0.4],
  ['resource', 'Wood_Plank_A', -0.4, 0.5, 2.1],
  ['resource', 'Wood_Log_A', 0.1, -0.5, 1.2],
];
const PIT_MODEL = ['halloween', 'floor_dirt_small'] as const;
const PIT_RADIUS = 1.3;

/**
 * The marks the villagers leave on the map's props and ground: a prop's pieces or ash where it stood,
 * a pit where someone dug, short grass where it was mown. The props and grass themselves are in the
 * merged scenery, which this hides by handle.
 */
export class Marks {
  readonly group = new THREE.Group();
  /** `p:<place>:<index>` -> the prop's handle in the merged scenery */
  private readonly props = new Map<string, SceneryHandle>();
  /** `h:<col>,<row>` -> the grass handles on that hex */
  private readonly grass = new Map<string, SceneryHandle[]>();
  /** what is drawn for each object id right now */
  private readonly drawn = new Map<string, THREE.Object3D>();
  private readonly ash: THREE.Mesh;
  private readonly pit: THREE.Object3D;
  private readonly mown: THREE.Mesh;
  private readonly pieces: THREE.Object3D;

  constructor(
    private readonly assets: Assets,
    private readonly builder: SceneryBuilder,
    private readonly heightAt: (x: number, z: number) => number,
    quality: QualitySettings,
  ) {
    this.group.name = 'marks';
    const disc = (radius: number, color: number, lift: number): THREE.Mesh => {
      const mesh = new THREE.Mesh(new THREE.CircleGeometry(radius, quality.quality === 'high' ? 18 : 10).rotateX(-Math.PI / 2), new THREE.MeshLambertMaterial({ color }));
      mesh.position.y = lift;
      mesh.receiveShadow = true;
      return mesh;
    };
    this.ash = disc(1.1, 0x2a241f, 0.03);
    this.mown = disc(5.5, 0x9ab24f, 0.015);
    const pit = new THREE.Group();
    pit.add(assets.model(...PIT_MODEL).clone(), disc(PIT_RADIUS * 0.7, 0x3d2e1f, -0.06));
    this.pit = pit;
    const pieces = new THREE.Group();
    for (const [bundle, model, dx, dz, turn] of PIECES) {
      if (!assets.hasModel(bundle, model)) continue;
      const piece = assets.model(bundle, model).clone();
      piece.position.set(dx, 0.02, dz);
      piece.rotation.set(0, turn, 0);
      pieces.add(piece);
    }
    this.pieces = pieces;
  }

  /** Remembers a prop's handle so it can be hidden when the prop is taken apart. */
  prop(placeId: string, index: number, handle: SceneryHandle): void {
    this.props.set(`p:${placeId}:${index}`, handle);
  }

  /** Remembers a grass tuft's handle under its hex so the hex can be mown. */
  grassOn(model: PlacedModel, handle: SceneryHandle): void {
    const hex = hexOf(model.x, model.z);
    const list = this.grass.get(hex);
    if (list) list.push(handle);
    else this.grass.set(hex, [handle]);
  }

  /** Brings the marks in line with the server's objects; props and ground it does not mention are as the map has them. */
  show(objects: ReadonlyMap<string, WorldObject>): void {
    const seen = new Set<string>();
    for (const object of objects.values()) {
      if (object.kind !== 'prop' && object.kind !== 'ground') continue;
      seen.add(object.id);
      const whole = object.stage === 'whole' || object.stage === 'burning';
      if (object.kind === 'prop') {
        const handle = this.props.get(object.id);
        if (handle !== undefined) this.builder.setVisible(handle, whole);
      } else {
        for (const handle of this.grass.get(object.id) ?? []) this.builder.setVisible(handle, object.stage !== 'mown');
      }
      const want = this.markFor(object);
      const have = this.drawn.get(object.id);
      if (have && have.name === want) continue;
      if (have) have.removeFromParent();
      this.drawn.delete(object.id);
      if (!want) continue;
      const mark = this.make(want, object.id);
      if (mark) {
        this.group.add(mark);
        this.drawn.set(object.id, mark);
      }
    }
    for (const [id, mark] of this.drawn) {
      if (seen.has(id)) continue;
      mark.removeFromParent();
      this.drawn.delete(id);
    }
    for (const [id, handle] of this.props) if (!seen.has(id)) this.builder.setVisible(handle, true);
    for (const [id, handles] of this.grass) if (!seen.has(id)) for (const handle of handles) this.builder.setVisible(handle, true);
  }

  private markFor(object: WorldObject): string {
    switch (object.stage) {
      case 'pieces':
        return 'pieces';
      case 'ash':
        return 'ash';
      case 'dug':
        return 'pit';
      case 'mown':
        return 'mown';
      default:
        return '';
    }
  }

  /** Where an object of the map stands, from the client's own copy of it. */
  private positionOf(id: string): { x: number; z: number; rotation: number } | null {
    if (id.startsWith('h:')) {
      const [col, row] = id.slice(2).split(',').map(Number) as [number, number];
      const cell = cellAt(col, row);
      return cell ? { x: cell.center.x, z: cell.center.z, rotation: 0 } : null;
    }
    if (id.startsWith('p:')) {
      const at = id.lastIndexOf(':');
      const place = places.find((entry) => entry.id === id.slice(2, at));
      const model = place?.models[Number(id.slice(at + 1))];
      return model ? { x: model.x, z: model.z, rotation: model.rotation } : null;
    }
    if (id.startsWith('s:')) {
      const model = scatter.models[Number(id.slice(2))];
      return model ? { x: model.x, z: model.z, rotation: model.rotation } : null;
    }
    return null;
  }

  private make(kind: string, id: string): THREE.Object3D | null {
    const at = this.positionOf(id);
    if (!at) return null;
    const template = kind === 'pieces' ? this.pieces : kind === 'ash' ? this.ash : kind === 'pit' ? this.pit : this.mown;
    const mark = template.clone();
    mark.name = kind;
    mark.position.set(at.x, this.heightAt(at.x, at.z), at.z);
    mark.rotation.y = kind === 'mown' ? 0 : at.rotation + (hashString(id) % 6) * 0.3;
    if (kind === 'pit') mark.position.x += ((hashString(id) % 5) - 2) * 0.8;
    return mark;
  }
}

function hexOf(x: number, z: number): string {
  const hex = hexAt(x, z);
  return `h:${hex.col},${hex.row}`;
}
