import { clamp, dist, type Vec2 } from '../../core/math2d';
import { secondsToTicks } from '../../core/time';
import { intRange, nextFloat, pick, range, weightedPick } from '../../core/rng';
import { nearestWalkable } from '../../world/navgrid';
import { getPlace } from '../../world/places';
import { animSeconds } from '../data/anims';
import { MONSTERS, type MonsterKind } from '../data/monsters';
import type { Character, Monster } from '../state';
import { newId } from '../state';
import { chronicle, getCharacter, living, monsterList, setAnim, type Ctx } from '../util';
import { damageCharacter } from './combat';
import { advance, face, routeTo } from './movement';

const MAX_MONSTERS = 9;
const SUMMON_SECONDS = 26;
const ROAM_TARGETS = ['green', 'tavern', 'market', 'watchtower', 'smithy', 'graveyard', 'graveyard'];

export function spawnMonster(ctx: Ctx, kind: MonsterKind, pos: Vec2, facing = 0): Monster {
  const { state } = ctx;
  const def = MONSTERS[kind];
  const monster: Monster = {
    id: newId(state, 'm'),
    kind,
    look: { model: def.model, texture: def.texture, hide: [], mainHand: def.mainHand, offHand: def.offHand },
    pos: { ...pos },
    facing,
    moving: false,
    hp: def.maxHp,
    phase: 'rising',
    sinceTick: state.tick,
    targetId: null,
    path: [],
    repathTick: 0,
    nextAttackTick: 0,
    hitTick: 0,
    hitTargetId: null,
    nextSummonTick: state.tick + secondsToTicks(SUMMON_SECONDS),
    anim: 'skeleton_spawn',
    animSeq: 0,
    stuck: 0,
  };
  state.monsters[monster.id] = monster;
  ctx.events.push({ type: 'monster_spawn', id: monster.id, pos: { ...pos } });
  return monster;
}

export function randomMonsterKind(ctx: Ctx): MonsterKind {
  const kinds = Object.values(MONSTERS).filter((def) => def.weight > 0);
  return (weightedPick(ctx.state, kinds, (def) => def.weight) ?? kinds[0]!).kind;
}

function findTarget(ctx: Ctx, monster: Monster, sight: number): Character | null {
  let best: Character | null = null;
  let bestDistance = sight;
  for (const c of living(ctx.state)) {
    if (c.inside) continue;
    const d = dist(c.pos, monster.pos) * (c.id === monster.targetId ? 0.75 : 1);
    if (d < bestDistance) {
      bestDistance = d;
      best = c;
    }
  }
  return best;
}

function die(ctx: Ctx, monster: Monster): void {
  monster.phase = 'dying';
  monster.sinceTick = ctx.state.tick;
  monster.hitTick = 0;
  monster.path = [];
  setAnim(monster, 'skeleton_death', true);
}

export function updateMonsters(ctx: Ctx): void {
  const { state } = ctx;
  const daylight = ctx.time.hour >= 5.6 && ctx.time.hour < 19.4;
  let crumbled = 0;

  for (const monster of monsterList(state)) {
    const def = MONSTERS[monster.kind];
    monster.moving = false;

    if (monster.phase === 'dying') {
      if (state.tick - monster.sinceTick > secondsToTicks(5)) delete state.monsters[monster.id];
      continue;
    }
    if (daylight) {
      die(ctx, monster);
      crumbled++;
      continue;
    }
    if (monster.phase === 'rising') {
      if (state.tick - monster.sinceTick >= secondsToTicks(animSeconds('skeleton_spawn'))) {
        monster.phase = 'roam';
        monster.sinceTick = state.tick;
      }
      continue;
    }

    // A blow already swinging lands first.
    if (monster.hitTick > 0) {
      const victim = getCharacter(state, monster.hitTargetId);
      if (victim) face(monster, victim.pos);
      if (state.tick >= monster.hitTick) {
        monster.hitTick = 0;
        const reach = def.range * (def.style === 'melee' ? 1.45 : 1.5);
        if (victim && !victim.inside && dist(victim.pos, monster.pos) <= reach) {
          damageCharacter(ctx, victim, intRange(state, def.damage[0], def.damage[1]), { id: monster.id, label: def.label, character: null }, 'monster');
        }
      }
      continue;
    }

    if ((state.tick + Number(monster.id.slice(1))) % 10 === 0) {
      const found = findTarget(ctx, monster, monster.targetId ? def.sight * 1.8 : def.sight);
      monster.targetId = found?.id ?? null;
      monster.phase = found ? 'chase' : 'roam';
    }
    const chased = getCharacter(state, monster.targetId);
    if (monster.phase === 'chase' && (!chased || chased.inside)) {
      monster.targetId = null;
      monster.phase = 'roam';
    }

    if (monster.kind === 'necromancer' && state.tick >= monster.nextSummonTick && monsterList(state).length < MAX_MONSTERS) {
      monster.nextSummonTick = state.tick + secondsToTicks(SUMMON_SECONDS);
      const spot = nearestWalkable({ x: monster.pos.x + range(state, -3, 3), z: monster.pos.z + range(state, -3, 3) }, 4);
      if (spot) {
        setAnim(monster, 'magic_summon', true);
        spawnMonster(ctx, 'skeleton_minion', spot, monster.facing);
        continue;
      }
    }

    const target = getCharacter(state, monster.targetId);
    if (monster.phase === 'chase' && target) {
      const d = dist(monster.pos, target.pos);
      if (d > def.range * 0.9) {
        if (monster.path.length === 0 || state.tick >= monster.repathTick) {
          monster.path = routeTo(monster.pos, target.pos) ?? [];
          monster.repathTick = state.tick + secondsToTicks(0.7);
        }
        advance(monster, monster.path, def.speed * 1.15);
        setAnim(monster, monster.kind === 'skeleton_rogue' ? 'run' : 'skeleton_walk');
      } else {
        monster.path = [];
        face(monster, target.pos);
        if (state.tick >= monster.nextAttackTick) {
          const anim = pick(state, def.attackAnims);
          setAnim(monster, anim, true);
          monster.hitTick = state.tick + secondsToTicks(clamp(animSeconds(anim) * 0.45, 0.35, 0.8));
          monster.hitTargetId = target.id;
          monster.nextAttackTick = state.tick + secondsToTicks(def.interval);
          ctx.events.push({
            type: 'attack', attackerId: monster.id, targetId: target.id,
            ranged: def.style === 'magic' ? 'magic' : null, from: { ...monster.pos }, to: { ...target.pos },
          });
        } else if (state.tick > monster.nextAttackTick - secondsToTicks(def.interval * 0.4)) {
          setAnim(monster, 'skeleton_idle');
        }
      }
      continue;
    }

    // Roaming: drift towards where the living are.
    if (monster.path.length === 0) {
      if (state.tick < monster.repathTick) {
        setAnim(monster, 'skeleton_idle');
        continue;
      }
      const place = getPlace(pick(state, ROAM_TARGETS));
      const goal = nearestWalkable({ x: place.door.x + range(state, -7, 7), z: place.door.z + range(state, -7, 7) }, 8);
      monster.path = goal ? (routeTo(monster.pos, goal) ?? []) : [];
      monster.repathTick = state.tick + secondsToTicks(3 + nextFloat(state) * 4);
    }
    if (monster.path.length > 0) {
      advance(monster, monster.path, def.speed * 0.8);
      setAnim(monster, 'skeleton_walk');
    }
  }

  if (crumbled > 0) chronicle(ctx, 'world', 'The first light of dawn turned the walking dead back to dust.');
}
