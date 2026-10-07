import type { Billboard, Dyn, DynDelta, Entity, EntityKind, Grave, House, Look, Snapshot, Stockpile, TickFrame, WorldObject } from '../generated/api';

/** One thing to draw: what the renderer needs each tick, decoded from the wire. */
export interface ViewEntity {
  id: string;
  kind: EntityKind;
  /** label over the head; empty for a corpse */
  name: string;
  look: Look;
  maxHp: number;
  hostile: boolean;
  x: number;
  z: number;
  facing: number;
  anim: string;
  animSeq: number;
  visible: boolean;
  /** what is in each hand right now, after the server's at-ease rule */
  mainHand: string | null;
  offHand: string | null;
  hp: number;
  /** current activity id, if performing one */
  act: string | null;
  asleep: boolean;
}

const graveKey = (grave: Grave): string => `${grave.plot}:${grave.characterId}`;

function decode(target: ViewEntity, dyn: Partial<Dyn>): void {
  if (dyn.p) {
    target.x = dyn.p[0] / 100;
    target.z = dyn.p[1] / 100;
  }
  if (dyn.f !== undefined) target.facing = dyn.f / 100;
  if (dyn.a) {
    target.anim = dyn.a[0];
    target.animSeq = dyn.a[1];
  }
  if (dyn.hp !== undefined) target.hp = dyn.hp;
  if (dyn.v !== undefined) target.visible = dyn.v;
  if (dyn.mh !== undefined) target.mainHand = dyn.mh;
  if (dyn.oh !== undefined) target.offHand = dyn.oh;
  if (dyn.act !== undefined) target.act = dyn.act;
  if (dyn.z !== undefined) target.asleep = dyn.z;
}

function fromEntity(entity: Entity): ViewEntity {
  const s = entity.static;
  const view: ViewEntity = {
    id: s.id, kind: s.kind, name: s.name, look: s.look, maxHp: s.maxHp, hostile: s.hostile,
    x: 0, z: 0, facing: 0, anim: 'idle', animSeq: 0, visible: true, mainHand: null, offHand: null, hp: 0, act: null, asleep: false,
  };
  decode(view, entity.dyn);
  return view;
}

/**
 * The world as this viewer has been told it: entities, graves, billboards, houses, what differs
 * from the map and the stockpiles, kept up to
 * date by applying each tick frame in order. Everything else (roster, chronicle,
 * voices) goes to the HUD store.
 */
export class ViewWorld {
  /** the last tick applied */
  tick = 0;
  readonly entities = new Map<string, ViewEntity>();
  /** keyed `plot:characterId` */
  readonly graves = new Map<string, Grave>();
  /** bumped when the graves change, so the renderer can skip unchanged ticks cheaply */
  gravesVersion = 0;
  /** what each billboard shows, by billboard id */
  readonly billboards = new Map<string, Billboard>();
  /** bumped when a billboard changes */
  billboardsVersion = 0;
  /** the homes that are burning or in ruins, by place id; a home not here stands whole */
  readonly houses = new Map<string, House>();
  /** bumped when a house changes */
  housesVersion = 0;
  /** everything that differs from the map (felled trees, mined rocks, ...), by object id */
  readonly worldObjects = new Map<string, WorldObject>();
  /** bumped when a world object changes */
  worldObjectsVersion = 0;
  /** the materials kept at each place that holds any, by place id */
  readonly stockpiles = new Map<string, Stockpile>();
  /** bumped when a stockpile changes */
  stockpilesVersion = 0;

  reset(snapshot: Snapshot): void {
    this.tick = snapshot.tick;
    this.entities.clear();
    for (const entity of snapshot.entities) this.entities.set(entity.static.id, fromEntity(entity));
    this.graves.clear();
    for (const grave of snapshot.graves) this.graves.set(graveKey(grave), grave);
    this.gravesVersion++;
    this.billboards.clear();
    for (const board of snapshot.billboards) this.billboards.set(board.id, board);
    this.billboardsVersion++;
    this.houses.clear();
    for (const house of snapshot.houses) this.houses.set(house.id, house);
    this.housesVersion++;
    this.worldObjects.clear();
    for (const object of snapshot.worldObjects) this.worldObjects.set(object.id, object);
    this.worldObjectsVersion++;
    this.stockpiles.clear();
    for (const pile of snapshot.stockpiles) this.stockpiles.set(pile.place, pile);
    this.stockpilesVersion++;
  }

  /** Applies one tick: additions, removals, changed fields, new graves, billboards and houses. */
  apply(frame: TickFrame): void {
    this.tick = frame.t;
    for (const entity of frame.add ?? []) this.entities.set(entity.static.id, fromEntity(entity));
    for (const id of frame.del ?? []) this.entities.delete(id);
    for (const delta of frame.d ?? []) this.applyDelta(delta);
    for (const event of frame.ev ?? []) {
      if (event.type === 'grave') this.addGrave(event.grave);
      else if (event.type === 'billboard') {
        this.billboards.set(event.billboard.id, event.billboard);
        this.billboardsVersion++;
      } else if (event.type === 'house') {
        if (event.house.phase === 'intact') this.houses.delete(event.house.id);
        else this.houses.set(event.house.id, event.house);
        this.housesVersion++;
      } else if (event.type === 'world_object') {
        if (event.object.stage === 'intact') this.worldObjects.delete(event.object.id);
        else this.worldObjects.set(event.object.id, event.object);
        this.worldObjectsVersion++;
      } else if (event.type === 'stockpile') {
        const pile = event.stockpile;
        if (pile.wood + pile.stone + pile.ore === 0) this.stockpiles.delete(pile.place);
        else this.stockpiles.set(pile.place, pile);
        this.stockpilesVersion++;
      }
    }
  }

  private applyDelta(delta: DynDelta): void {
    const entity = this.entities.get(delta.id);
    if (entity) decode(entity, delta);
  }

  /** A full graveyard reuses its oldest plot, so a new grave replaces whatever lay there. */
  private addGrave(grave: Grave): void {
    for (const [key, existing] of this.graves) if (existing.plot === grave.plot) this.graves.delete(key);
    this.graves.set(graveKey(grave), grave);
    this.gravesVersion++;
  }

  isVillager(id: string): boolean {
    return this.entities.get(id)?.kind === 'villager';
  }

  isMonster(id: string): boolean {
    return this.entities.get(id)?.kind === 'monster';
  }
}
