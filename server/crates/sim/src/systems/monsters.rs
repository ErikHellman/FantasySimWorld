//! The walking dead: rising, roaming, chasing, striking, crumbling at dawn.

use fsw_core::math2d::{clamp, dist};
use fsw_core::rng;
use fsw_core::time::seconds_to_ticks;
use fsw_core::{Vec2, vec2};
use fsw_world::places::get_place;

use crate::data::anims::AnimId;
use crate::data::classes::WeaponStyle;
use crate::data::monsters::{MONSTERS, MonsterKind};
use crate::events::{Projectile, SimEvent};
use crate::state::{ChronicleKind, FightMode, Id, Look, Monster, MonsterPhase};
use crate::systems::combat::{Assailant, damage_character};
use crate::systems::fire::arson;
use crate::systems::movement::{advance, face, route_to};
use crate::util::{Ctx, living, monster_ids, set_anim};

/// The most of the dead abroad at once, the dying included.
pub const MAX_MONSTERS: usize = 14;
const SUMMON_SECONDS: f64 = 20.0;
const SUMMONED_PER_CAST: usize = 2;
const ROAM_TARGETS: [&str; 7] = ["green", "tavern", "market", "watchtower", "smithy", "graveyard", "graveyard"];

pub fn spawn_monster(ctx: &mut Ctx, kind: MonsterKind, pos: Vec2, facing: f64) -> Id {
    let def = kind.def();
    let tick = ctx.tick();
    let id = ctx.state.new_id("m");
    let monster = Monster {
        id: id.clone(),
        kind,
        look: Look {
            model: def.model.into(),
            texture: def.texture.map(str::to_owned),
            hide: Vec::new(),
            main_hand: def.main_hand.map(str::to_owned),
            off_hand: def.off_hand.map(str::to_owned),
        },
        pos,
        facing,
        moving: false,
        hp: def.max_hp,
        phase: MonsterPhase::Rising,
        since_tick: tick,
        target_id: None,
        path: Vec::new(),
        repath_tick: 0,
        next_attack_tick: 0,
        hit_tick: 0,
        hit_target_id: None,
        next_summon_tick: tick + seconds_to_ticks(SUMMON_SECONDS),
        anim: AnimId::SkeletonSpawn,
        anim_seq: 0,
        stuck: 0,
        name: None,
        lasting: false,
        was_id: None,
        until_tick: None,
        arson: None,
    };
    ctx.state.monsters.insert(id.clone(), monster);
    ctx.emit(SimEvent::MonsterSpawn { id: id.clone(), pos });
    id
}

/// By day, one of the dead who does not crumble keeps to the ruins, unless
/// someone has come to fight it there. True while it is lying low.
fn lie_low(ctx: &mut Ctx, id: &str) -> bool {
    let tick = ctx.tick();
    let monster = &ctx.state.monsters[id];
    let def = monster.kind.def();
    let challenger = monster
        .target_id
        .as_deref()
        .and_then(|t| ctx.state.characters.get(t))
        .filter(|c| c.inside.is_none() && dist(c.pos, monster.pos) < def.sight * 1.8)
        .is_some();
    if challenger && monster.phase == MonsterPhase::Chase {
        return false;
    }
    let lair = get_place("ruins").door;
    let nav = ctx.state.nav.clone();
    let monster = ctx.m(id);
    monster.target_id = None;
    monster.phase = MonsterPhase::Roam;
    if dist(monster.pos, lair) < 6.0 {
        monster.path.clear();
        set_anim(monster, AnimId::SkeletonIdle, false);
        return true;
    }
    if monster.path.is_empty() || tick >= monster.repath_tick {
        monster.path = route_to(&nav, monster.pos, lair).unwrap_or_default();
        monster.repath_tick = tick + seconds_to_ticks(5.0);
    }
    let mut path = std::mem::take(&mut monster.path);
    advance(monster, &mut path, def.speed * 0.8);
    monster.path = path;
    set_anim(monster, AnimId::SkeletonWalk, false);
    true
}

pub fn random_monster_kind(ctx: &mut Ctx) -> MonsterKind {
    let kinds: Vec<_> = MONSTERS.iter().filter(|def| def.weight > 0.0).collect();
    let index = rng::weighted_pick_index(&mut ctx.state.rng, &kinds, |def| def.weight).unwrap_or(0);
    kinds[index].kind
}

fn find_target(ctx: &Ctx, monster: &Monster, sight: f64) -> Option<Id> {
    let mut best = None;
    let mut best_distance = sight;
    for c in living(ctx.state) {
        if c.inside.is_some() {
            continue;
        }
        let bias = if monster.target_id.as_deref() == Some(c.id.as_str()) { 0.75 } else { 1.0 };
        let d = dist(c.pos, monster.pos) * bias;
        if d < best_distance {
            best_distance = d;
            best = Some(c.id.clone());
        }
    }
    best
}

fn die(tick: i64, monster: &mut Monster) {
    monster.phase = MonsterPhase::Dying;
    monster.since_tick = tick;
    monster.hit_tick = 0;
    monster.path.clear();
    set_anim(monster, AnimId::SkeletonDeath, true);
}

/// The number in a monster id ("m12" → 12), as `Number(id.slice(1))`.
fn id_number(id: &str) -> i64 {
    id.get(1..).and_then(|n| n.parse().ok()).unwrap_or(0)
}

pub fn update_monsters(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let daylight = ctx.time.hour >= 5.6 && ctx.time.hour < 19.4;
    let mut crumbled = 0;

    for id in monster_ids(ctx.state) {
        let Some(monster) = ctx.state.monsters.get_mut(&id) else { continue };
        let def = monster.kind.def();
        monster.moving = false;

        if monster.phase == MonsterPhase::Dying {
            if tick - monster.since_tick > seconds_to_ticks(5.0) {
                ctx.state.monsters.shift_remove(&id);
            }
            continue;
        }
        if monster.until_tick.is_some_and(|until| tick >= until) {
            die(tick, monster);
            continue;
        }
        let bought = monster.until_tick.is_some();
        if daylight && !monster.lasting && !bought {
            die(tick, monster);
            crumbled += 1;
            continue;
        }
        if monster.phase == MonsterPhase::Rising {
            if tick - monster.since_tick >= seconds_to_ticks(AnimId::SkeletonSpawn.seconds()) {
                monster.phase = MonsterPhase::Roam;
                monster.since_tick = tick;
            }
            continue;
        }
        if daylight && !bought && lie_low(ctx, &id) {
            continue;
        }
        let monster = ctx.m(&id);

        // A blow already swinging lands first.
        if monster.hit_tick > 0 {
            let victim = monster.hit_target_id.clone().filter(|v| ctx.state.characters.contains_key(v));
            let victim_pos = victim.as_ref().map(|v| ctx.state.characters[v].pos);
            let monster = ctx.m(&id);
            if let Some(pos) = victim_pos {
                face(monster, pos);
            }
            if tick >= monster.hit_tick {
                monster.hit_tick = 0;
                let reach = def.range * if def.style == WeaponStyle::Melee { 1.45 } else { 1.5 };
                let monster_pos = monster.pos;
                if let Some(victim) = victim
                    && ctx.state.characters[&victim].inside.is_none()
                    && dist(ctx.state.characters[&victim].pos, monster_pos) <= reach
                {
                    let damage = ctx.int_range(def.damage[0], def.damage[1]);
                    let by = Assailant { id: id.clone(), label: def.label.into(), character: None };
                    damage_character(ctx, &victim, damage, by, FightMode::Monster);
                }
            }
            continue;
        }

        if (tick + id_number(&id)) % 10 == 0 {
            let sight = if monster.target_id.is_some() { def.sight * 1.8 } else { def.sight };
            let found = find_target(ctx, &ctx.state.monsters[&id], sight);
            let monster = ctx.m(&id);
            monster.phase = if found.is_some() { MonsterPhase::Chase } else { MonsterPhase::Roam };
            monster.target_id = found;
        }
        let chased_gone = {
            let monster = &ctx.state.monsters[&id];
            let chased = monster.target_id.as_deref().and_then(|t| ctx.state.characters.get(t));
            monster.phase == MonsterPhase::Chase && chased.is_none_or(|c| c.inside.is_some())
        };
        if chased_gone {
            let monster = ctx.m(&id);
            monster.target_id = None;
            monster.phase = MonsterPhase::Roam;
        }

        let monster = &ctx.state.monsters[&id];
        if monster.kind == MonsterKind::Necromancer
            && (!daylight || bought)
            && tick >= monster.next_summon_tick
            && ctx.state.monsters.len() < MAX_MONSTERS
        {
            let (pos, facing, until_tick) = (monster.pos, monster.facing, monster.until_tick);
            ctx.m(&id).next_summon_tick = tick + seconds_to_ticks(SUMMON_SECONDS);
            let mut summoned = false;
            for _ in 0..SUMMONED_PER_CAST {
                if ctx.state.monsters.len() >= MAX_MONSTERS {
                    break;
                }
                let x = pos.x + ctx.range(-3.0, 3.0);
                let z = pos.z + ctx.range(-3.0, 3.0);
                if let Some(spot) = ctx.state.nav.nearest_walkable(vec2(x, z), 4) {
                    let minion = spawn_monster(ctx, MonsterKind::SkeletonMinion, spot, facing);
                    // What a bought necromancer raises goes when it does.
                    ctx.m(&minion).until_tick = until_tick;
                    summoned = true;
                }
            }
            if summoned {
                set_anim(ctx.m(&id), AnimId::MagicSummon, true);
                continue;
            }
        }

        let monster = &ctx.state.monsters[&id];
        let target =
            monster.target_id.as_deref().and_then(|t| ctx.state.characters.get(t)).map(|c| (c.id.clone(), c.pos));
        if monster.phase == MonsterPhase::Chase
            && let Some((target_id, target_pos)) = target
        {
            let monster = ctx.m(&id);
            let d = dist(monster.pos, target_pos);
            if d > def.range * 0.9 {
                if monster.path.is_empty() || tick >= monster.repath_tick {
                    monster.path = route_to(&nav, monster.pos, target_pos).unwrap_or_default();
                    monster.repath_tick = tick + seconds_to_ticks(0.7);
                }
                let mut path = std::mem::take(&mut monster.path);
                advance(monster, &mut path, def.speed * 1.15);
                monster.path = path;
                let anim = if monster.kind == MonsterKind::SkeletonRogue { AnimId::Run } else { AnimId::SkeletonWalk };
                set_anim(monster, anim, false);
            } else {
                monster.path.clear();
                face(monster, target_pos);
                if tick >= monster.next_attack_tick {
                    let anim = ctx.pick(def.attack_anims);
                    let monster = ctx.m(&id);
                    set_anim(monster, anim, true);
                    monster.hit_tick = tick + seconds_to_ticks(clamp(anim.seconds() * 0.45, 0.35, 0.8));
                    monster.hit_target_id = Some(target_id.clone());
                    monster.next_attack_tick = tick + seconds_to_ticks(def.interval);
                    let from = monster.pos;
                    ctx.emit(SimEvent::Attack {
                        attacker_id: id.clone(),
                        target_id,
                        ranged: if def.style == WeaponStyle::Magic { Some(Projectile::Magic) } else { None },
                        from,
                        to: target_pos,
                    });
                } else if tick > monster.next_attack_tick - seconds_to_ticks(def.interval * 0.4) {
                    set_anim(monster, AnimId::SkeletonIdle, false);
                }
            }
            continue;
        }

        // With the whole village asleep, a roaming monster goes for a house instead.
        if arson(ctx, &id, (tick + id_number(&id)) % 10 == 0) {
            continue;
        }

        // Roaming: drift towards where the living are.
        let monster = ctx.m(&id);
        if monster.path.is_empty() {
            if tick < monster.repath_tick {
                set_anim(monster, AnimId::SkeletonIdle, false);
                continue;
            }
            let place = get_place(ROAM_TARGETS[ctx.pick_index(ROAM_TARGETS.len())]);
            let x = place.door.x + ctx.range(-7.0, 7.0);
            let z = place.door.z + ctx.range(-7.0, 7.0);
            let goal = nav.nearest_walkable(vec2(x, z), 8);
            let pos = ctx.state.monsters[&id].pos;
            let path = goal.and_then(|goal| route_to(&nav, pos, goal)).unwrap_or_default();
            let wait = 3.0 + ctx.next_float() * 4.0;
            let monster = ctx.m(&id);
            monster.path = path;
            monster.repath_tick = tick + seconds_to_ticks(wait);
        }
        let monster = ctx.m(&id);
        if !monster.path.is_empty() {
            let mut path = std::mem::take(&mut monster.path);
            advance(monster, &mut path, def.speed * 0.8);
            monster.path = path;
            set_anim(monster, AnimId::SkeletonWalk, false);
        }
    }

    if crumbled > 0 {
        ctx.chronicle(
            ChronicleKind::World,
            "The first light of dawn turned the walking dead back to dust.",
            Vec::new(),
        );
    }
}
