//! Fights: with the dead, brawls, and the rare duel to the death.

use fsw_core::Vec2;
use fsw_core::jsmath::js_round;
use fsw_core::math2d::{clamp, dist};
use fsw_core::time::seconds_to_ticks;

use crate::data::anims::AnimId;
use crate::data::classes::{FISTS, RUN_SPEED, Weapon, WeaponStyle};
use crate::events::{Emote, Projectile, SimEvent};
use crate::state::{
    Act, ActPhase, ChronicleKind, FightMode, Id, Intent, IntentAction, IntentSource, MemoryKind, MonsterKind,
    MonsterPhase, Quirk, StimulusKind,
};
use crate::systems::charm;
use crate::systems::conversation::abort_conversation;
use crate::systems::death::{detach, free_grave_plot, kill_character};
use crate::systems::economy::drop_loot;
use crate::systems::ghosts;
use crate::systems::movement::{advance, face, route_to};
use crate::systems::quirks::{ambushed, end_hunt, escaped};
use crate::systems::relationships::{RelationDelta, adjust};
use crate::util::{
    Ctx, bump_epoch, first_name, get_monster, has_recent_stimulus, is_fighting, nearby_characters, place_name_at,
    set_anim, their,
};

const BRAWL_YIELD: f64 = 0.3;
const LETHAL_YIELD: f64 = 0.25;
const POTION_HEAL: f64 = 55.0;
/// A blow from behind, on someone who never saw it coming.
pub const SNEAK_ATTACK_MULTIPLIER: i64 = 5;
/// How much harder a born fighter hits than their class would.
const FIGHTER_DAMAGE: f64 = 1.6;
/// The share of blows from the dead that break a coward's nerve.
const COWARD_PANIC: f64 = 0.5;
/// The most anyone panics when the dead land a blow; the timid come close to it, the fearless hardly ever.
pub const PANIC_MAX: f64 = 0.15;
/// A villager the model told to stand runs at this share of the usual breaking point.
pub const LAST_RESORT: f64 = 0.5;
const PANIC_LINES: [&str; 4] = [
    "I can't, I can't do this!",
    "Gods help me, I'm getting out of here!",
    "Run! Everyone run!",
    "No, no, no! I'm going home!",
];

/// Hit points when whole: the class's, or a child's share of them.
pub fn max_hp(c: &crate::state::Character) -> f64 {
    let class = c.class_id.def().max_hp;
    if c.is_child() { class * crate::data::children::CHILD_HP_FRACTION } else { class }
}

pub fn weapon_of(c: &crate::state::Character, mode: FightMode) -> &'static Weapon {
    if mode == FightMode::Brawl { &FISTS } else { &c.class_id.def().weapon }
}

fn projectile_of(weapon: &Weapon) -> Option<Projectile> {
    match weapon.style {
        WeaponStyle::Bow => Some(Projectile::Arrow),
        WeaponStyle::Crossbow => Some(Projectile::Bolt),
        WeaponStyle::Magic => Some(Projectile::Magic),
        WeaponStyle::Melee => None,
    }
}

fn is_one_shot_playing(anim: AnimId, start_tick: i64, tick: i64) -> bool {
    !anim.loops() && tick < start_tick + seconds_to_ticks(anim.seconds())
}

fn fresh(tick: i64) -> Act {
    Act {
        phase: ActPhase::Start,
        path: Vec::new(),
        dest: None,
        since_tick: tick,
        until_tick: 0,
        repath_tick: 0,
        stuck: 0,
    }
}

/// Replaces whatever the character was doing with a fight.
pub fn start_fight(
    ctx: &mut Ctx,
    id: &str,
    target_id: &str,
    mode: FightMode,
    source: IntentSource,
    say: Option<String>,
) {
    detach(ctx, id);
    let tick = ctx.tick();
    let c = ctx.c(id);
    if c.inside.is_some() {
        c.inside = None;
    }
    c.asleep = false;
    c.tool = None;
    c.queue.clear();
    c.intent = Some(Intent {
        action: IntentAction::Fight { target_id: target_id.to_owned(), mode },
        source,
        say,
        thought: None,
    });
    c.act = fresh(tick);
    c.combat.target_id = Some(target_id.to_owned());
    c.combat.mode = mode;
    c.combat.hit_tick = 0;
    bump_epoch(c);
}

pub fn start_flee(ctx: &mut Ctx, id: &str, source: IntentSource) {
    detach(ctx, id);
    let tick = ctx.tick();
    let c = ctx.c(id);
    // Someone dozing in the open wakes with a start.
    if c.inside.is_none() {
        c.asleep = false;
    }
    c.tool = None;
    c.queue.clear();
    c.intent = Some(Intent::new(IntentAction::Flee, source));
    c.act = fresh(tick);
    c.combat.target_id = None;
    c.combat.hit_tick = 0;
    bump_epoch(c);
}

fn end_fight(ctx: &mut Ctx, id: &str, outcome: String) {
    let c = ctx.c(id);
    c.outcome = outcome;
    c.act.phase = ActPhase::Done;
    c.act.path.clear();
    c.combat.target_id = None;
    c.combat.hit_tick = 0;
}

/// Someone who is hit while minding their own business hits back, or runs.
fn reflex_defend(ctx: &mut Ctx, victim: &str, attacker_id: &str, mode: FightMode) {
    let v = ctx.cr(victim);
    if is_fighting(v) || v.combat.yielded {
        return;
    }
    let hp_left = v.hp / max_hp(v);
    let stands = v.profile.bravery * 0.75 + hp_left * 0.35 + if mode == FightMode::Brawl { 0.2 } else { 0.0 } > 0.62;
    // A child always runs.
    let stands = !v.has(Quirk::Coward) && !v.is_child() && (stands || v.has(Quirk::Fighter));
    if stands {
        start_fight(ctx, victim, attacker_id, mode, IntentSource::Reflex, None);
    } else {
        start_flee(ctx, victim, IntentSource::Reflex);
    }
}

/// How likely each blow from the dead is to break this villager's nerve: mostly a matter of bravery, and a hot
/// temper steadies a little. Never more than `PANIC_MAX`.
pub fn panic_chance(c: &crate::state::Character) -> f64 {
    if c.has(Quirk::Coward) {
        return COWARD_PANIC;
    }
    if c.has(Quirk::Fighter) {
        return 0.0;
    }
    PANIC_MAX * clamp(1.0 - c.profile.bravery * 0.85 - c.profile.temper * 0.15, 0.0, 1.0)
}

/// Someone fighting the dead loses their nerve and runs for home.
fn panic(ctx: &mut Ctx, id: &str) {
    let pos = ctx.cr(id).pos;
    let where_ = place_name_at(pos);
    start_flee(ctx, id, IntentSource::Reflex);
    let line = ctx.pick(&PANIC_LINES);
    ctx.say(id, line, None, true);
    ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Alert });
    ctx.remember(id, format!("I panicked and ran from the dead at {where_}."), 6.0, MemoryKind::Event, Vec::new());
    let name = ctx.cr(id).profile.name.clone();
    ctx.chronicle(
        ChronicleKind::Combat,
        format!("{name} panicked and fled home from the fight at {where_}."),
        vec![id.into()],
    );
}

pub fn damage_monster(ctx: &mut Ctx, monster_id: &str, amount: i64, attacker: &str) {
    let tick = ctx.tick();
    let attacker_pos = ctx.cr(attacker).pos;
    let current_pos = {
        let m = &ctx.state.monsters[monster_id];
        m.target_id.as_deref().and_then(|t| ctx.state.characters.get(t)).map(|c| c.pos)
    };
    let m = ctx.m(monster_id);
    let def = m.kind.def();
    m.hp -= amount as f64;
    let fatal = m.hp <= 0.0;
    let pos = m.pos;
    ctx.emit(SimEvent::Hit {
        attacker_id: attacker.into(),
        target_id: monster_id.into(),
        damage: amount as f64,
        pos,
        fatal,
    });
    let m = ctx.m(monster_id);
    if !fatal {
        if m.hit_tick == 0 {
            set_anim(m, AnimId::Hit, true);
        }
        if current_pos.is_none_or(|current| dist(m.pos, attacker_pos) < dist(m.pos, current)) {
            m.target_id = Some(attacker.to_owned());
        }
        if m.phase == MonsterPhase::Roam {
            m.phase = MonsterPhase::Chase;
        }
        return;
    }
    m.phase = MonsterPhase::Dying;
    m.since_tick = tick;
    m.hit_tick = 0;
    m.path.clear();
    set_anim(m, AnimId::SkeletonDeath, true);
    let kind = m.kind;
    ctx.c(attacker).kills += 1;
    drop_loot(ctx, monster_id, kind, pos, attacker);
    let where_ = place_name_at(pos);
    let necromancer = kind == MonsterKind::Necromancer;
    ctx.remember(
        attacker,
        format!("Destroyed a {} at {where_}.", def.label),
        if necromancer { 8.0 } else { 4.0 },
        MemoryKind::Event,
        Vec::new(),
    );
    let (fallen_name, was_id) = {
        let m = &ctx.state.monsters[monster_id];
        (m.name.clone(), m.was_id.clone())
    };
    if let Some(was_id) = was_id {
        // One of their own, lost to the dark, laid to rest at last.
        let name = ctx.cr(attacker).profile.name.clone();
        let fallen = fallen_name.unwrap_or_else(|| "the necromancer".into());
        ctx.chronicle(
            ChronicleKind::Combat,
            format!("{name} destroyed {fallen} at {where_}. The dark has let go of the valley, for now."),
            vec![attacker.into(), was_id.clone()],
        );
        bury_the_fallen(ctx, &was_id);
    } else if necromancer {
        let name = ctx.cr(attacker).profile.name.clone();
        ctx.chronicle(
            ChronicleKind::Combat,
            format!("{name} struck down the necromancer at {where_}!"),
            vec![attacker.into()],
        );
    }
    ctx.emit(SimEvent::Death { id: monster_id.into(), name: def.label.into(), pos, monster: true });
}

/// A grave, at last, for a villager who became one of the dead.
fn bury_the_fallen(ctx: &mut Ctx, was_id: &str) {
    let Some(record) = ctx.state.dead.get(was_id).cloned() else { return };
    let plot = free_grave_plot(ctx);
    let day = ctx.time.day;
    ctx.state.graves.push(crate::state::Grave {
        plot,
        character_id: was_id.to_owned(),
        name: record.name,
        day,
        cause: record.cause,
    });
    crate::systems::ceremonies::on_buried(ctx, was_id);
}

/// Who struck the blow.
pub struct Assailant {
    pub id: Id,
    pub label: String,
    /// set when a villager did it
    pub character: Option<Id>,
}

pub fn damage_character(ctx: &mut Ctx, victim: &str, amount: i64, by: Assailant, mode: FightMode) {
    let tick = ctx.tick();
    let v = ctx.c(victim);
    let cap = max_hp(v);
    let was_fresh = tick - v.combat.last_hit_by_tick > seconds_to_ticks(8.0);
    v.hp = if mode == FightMode::Brawl { (v.hp - amount as f64).max(1.0) } else { v.hp - amount as f64 };
    v.combat.last_hit_by_tick = tick;
    v.combat.last_hit_by_id = Some(by.id.clone());
    let fatal = v.hp <= 0.0;
    let pos = v.pos;
    ctx.emit(SimEvent::Hit { attacker_id: by.id.clone(), target_id: victim.into(), damage: amount as f64, pos, fatal });

    if fatal {
        let cause = match &by.character {
            Some(_) => format!("killed by {}", by.label),
            None => format!("slain by a {} at {}", by.label, place_name_at(pos)),
        };
        // Some of those the dead cut down do not rest at once; see `systems::ghosts`.
        let ghost = (mode == FightMode::Monster
            && by.character.is_none()
            && !ctx.cr(victim).is_child()
            && ghosts::returns(ctx.state.seed, victim))
        .then(|| ghosts::Haunt::of(ctx, victim));
        kill_character(ctx, victim, &cause, by.character.as_deref());
        if let Some(haunt) = ghost {
            ghosts::bind(ctx, haunt);
        }
        return;
    }

    if let Some(convo) = ctx.cr(victim).convo_id.clone() {
        abort_conversation(ctx, &convo);
    }
    let v = ctx.c(victim);
    if v.combat.hit_tick == 0 {
        set_anim(v, AnimId::Hit, true);
    }
    if was_fresh {
        bump_epoch(v);
        if !has_recent_stimulus(v, StimulusKind::Attacked, tick - seconds_to_ticks(20.0), Some(&by.id)) {
            let text = if by.character.is_some() {
                let how = if mode == FightMode::Brawl { " with their fists" } else { " with a weapon" };
                format!("{} is attacking you{how}!", by.label)
            } else {
                format!("A {} is attacking you!", by.label)
            };
            ctx.stimulate(victim, StimulusKind::Attacked, text, 3, Some(&by.id));
        }
    }
    if mode == FightMode::Monster {
        let v = ctx.cr(victim);
        let fighting_the_dead = !v.combat.yielded
            && matches!(
                v.intent.as_ref().map(|i| &i.action),
                Some(IntentAction::Fight { mode: FightMode::Monster, .. })
            );
        let chance = panic_chance(v);
        if fighting_the_dead && ctx.chance(chance) {
            panic(ctx, victim);
            return;
        }
    }
    reflex_defend(ctx, victim, &by.id, mode);

    let Some(attacker) = by.character else { return };
    // A murderer does not stop to hear anyone beg.
    let merciless = ctx.cr(&attacker).has(Quirk::Murderous);
    let v = ctx.cr(victim);
    if mode == FightMode::Brawl && v.hp <= cap * BRAWL_YIELD {
        end_brawl(ctx, &attacker, victim);
    } else if mode == FightMode::Lethal && v.hp <= cap * LETHAL_YIELD && !v.combat.yielded && !merciless {
        // Beaten: the victim yields and the attacker must choose.
        let v = ctx.c(victim);
        v.combat.yielded = true;
        v.intent = None;
        v.queue.clear();
        v.act.path.clear();
        v.combat.hit_tick = 0;
        set_anim(v, AnimId::SitFloor, false);
        ctx.say(victim, "Enough! I yield!", Some(&attacker), true);
        let wait = ctx.state.tuning.mercy_wait_ticks.max(1);
        let a = ctx.c(&attacker);
        a.combat.mercy_until_tick = tick + wait;
        a.combat.mercy_target_id = Some(victim.to_owned());
        a.combat.hit_tick = 0;
        let stance = weapon_of(a, FightMode::Lethal).stance;
        set_anim(a, stance, false);
        bump_epoch(a);
        ctx.emit(SimEvent::MercyRequest { attacker_id: attacker, victim_id: victim.into() });
    }
}

/// Those awake out of doors near enough to see what happens at `pos`.
pub fn witnesses(ctx: &Ctx, pos: Vec2, exclude: &[&str]) -> Vec<Id> {
    nearby_characters(ctx.state, pos, 22.0, None)
        .into_iter()
        .filter(|id| !exclude.contains(&id.as_str()) && !ctx.cr(id).asleep)
        .collect()
}

fn end_brawl(ctx: &mut Ctx, winner: &str, loser: &str) {
    let tick = ctx.tick();
    let starter =
        if ctx.cr(winner).intent.as_ref().map(|i| i.source) != Some(IntentSource::Reflex) { winner } else { loser };
    let loser_pos = ctx.cr(loser).pos;
    let where_ = place_name_at(loser_pos);
    let (w, l, s) = (
        first_name(ctx.cr(winner)).to_owned(),
        first_name(ctx.cr(loser)).to_owned(),
        first_name(ctx.cr(starter)).to_owned(),
    );
    end_fight(ctx, winner, format!("You beat {l} in a brawl."));
    end_fight(ctx, loser, format!("{w} beat you in a brawl."));
    let lc = ctx.c(loser);
    lc.intent = Some(Intent::new(IntentAction::Wait { minutes: 20.0 }, IntentSource::Reflex));
    lc.act = fresh(tick);
    set_anim(lc, AnimId::SitFloor, false);
    adjust(ctx, loser, winner, RelationDelta { grievance: 14.0, affinity: -10.0, trust: -10.0, ..Default::default() });
    adjust(ctx, winner, loser, RelationDelta { grievance: -22.0, affinity: -3.0, ..Default::default() });
    ctx.remember(
        winner,
        format!("Fought {l} with my fists at {where_}, and won."),
        7.0,
        MemoryKind::Event,
        vec![loser.into()],
    );
    ctx.remember(loser, format!("{w} beat me bloody at {where_}."), 8.0, MemoryKind::Event, vec![winner.into()]);
    let other = if starter == winner { &l } else { &w };
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!("{s} came to blows with {other} at {where_}; {w} won."),
        vec![winner.into(), loser.into()],
    );
    ctx.emit(SimEvent::Emote { id: loser.into(), emote: Emote::Anger });
    for witness in witnesses(ctx, loser_pos, &[winner, loser]) {
        adjust(ctx, &witness, starter, RelationDelta { affinity: -7.0, trust: -5.0, ..Default::default() });
        ctx.remember(
            &witness,
            format!("Watched {w} and {l} brawl at {where_}. {s} started it."),
            6.0,
            MemoryKind::Event,
            vec![winner.into(), loser.into()],
        );
        ctx.stimulate(
            &witness,
            StimulusKind::Witness,
            format!("You saw {w} beat {l} in a fist fight that {s} started."),
            1,
            Some(starter),
        );
    }
    charm::revel(ctx, winner, loser, true);
}

/// The attacker's answer once a beaten opponent has yielded.
pub fn resolve_mercy(ctx: &mut Ctx, attacker: &str, spare: bool, line: Option<String>) {
    let tick = ctx.tick();
    let a = ctx.c(attacker);
    let victim = a.combat.mercy_target_id.take();
    a.combat.mercy_until_tick = 0;
    let Some(victim) = victim.filter(|v| ctx.alive(v)) else {
        end_fight(ctx, attacker, "Your opponent is gone.".into());
        return;
    };
    let where_ = place_name_at(ctx.cr(&victim).pos);
    let (an, vn) = (first_name(ctx.cr(attacker)).to_owned(), first_name(ctx.cr(&victim)).to_owned());
    if !spare {
        if let Some(line) = line.filter(|l| !l.is_empty()) {
            ctx.say(attacker, line, Some(&victim), true);
        }
        end_fight(ctx, attacker, format!("You killed {vn}."));
        let anims = weapon_of(ctx.cr(attacker), FightMode::Lethal).anims;
        let anim = ctx.pick(anims);
        set_anim(ctx.c(attacker), anim, true);
        let cause = format!("killed by {}", ctx.cr(attacker).profile.name);
        kill_character(ctx, &victim, &cause, Some(attacker));
        return;
    }
    let line = line.filter(|l| !l.is_empty()).unwrap_or_else(|| "Get out of my sight.".into());
    ctx.say(attacker, line, Some(&victim), false);
    end_fight(ctx, attacker, format!("You had {vn} at your mercy and let them live."));
    ctx.c(&victim).combat.yielded = false;
    ctx.state.director.last_lethal_tick = tick;
    start_flee(ctx, &victim, IntentSource::Reflex);
    adjust(
        ctx,
        &victim,
        attacker,
        RelationDelta { grievance: 48.0, affinity: -60.0, trust: -55.0, ..Default::default() },
    );
    adjust(ctx, attacker, &victim, RelationDelta { grievance: -45.0, ..Default::default() });
    ctx.remember(
        attacker,
        format!("I drew steel on {vn} at {where_}. They yielded, and I let them live."),
        9.0,
        MemoryKind::Event,
        vec![victim.clone()],
    );
    ctx.remember(
        &victim,
        format!("{an} tried to kill me at {where_}. I begged, and was spared."),
        10.0,
        MemoryKind::Event,
        vec![attacker.into()],
    );
    let (a_full, v_full) = (ctx.cr(attacker).profile.name.clone(), ctx.cr(&victim).profile.name.clone());
    ctx.chronicle(
        ChronicleKind::Conflict,
        format!("{a_full} attacked {v_full} with a weapon at {where_}, but spared them."),
        vec![attacker.into(), victim.clone()],
    );
    let victim_pos = ctx.cr(&victim).pos;
    for witness in witnesses(ctx, victim_pos, &[attacker, &victim]) {
        adjust(
            ctx,
            &witness,
            attacker,
            RelationDelta { affinity: -25.0, trust: -30.0, grievance: 10.0, ..Default::default() },
        );
        ctx.remember(
            &witness,
            format!("Saw {an} nearly kill {vn} at {where_}."),
            8.0,
            MemoryKind::Event,
            vec![attacker.into(), victim.clone()],
        );
        ctx.stimulate(
            &witness,
            StimulusKind::Witness,
            format!("You saw {an} attack {vn} with a weapon; {vn} yielded and was spared."),
            2,
            Some(attacker),
        );
    }
}

fn land_hit(ctx: &mut Ctx, id: &str, weapon: &Weapon, mode: FightMode) {
    let c = ctx.c(id);
    let target = c.combat.hit_target_id.take();
    let pos = c.pos;
    let reach = weapon.range * if weapon.style == WeaponStyle::Melee { 1.45 } else { 1.5 };
    let fighter = c.has(Quirk::Fighter);
    let damage = ctx.int_range(weapon.damage[0], weapon.damage[1]);
    let damage = if fighter { js_round(damage as f64 * FIGHTER_DAMAGE) as i64 } else { damage };
    if mode == FightMode::Monster {
        if let Some(m) = get_monster(ctx.state, target.as_deref())
            && dist(pos, m.pos) <= reach
        {
            let monster = m.id.clone();
            damage_monster(ctx, &monster, damage, id);
        }
        return;
    }
    let Some(victim) = target.filter(|t| ctx.alive(t)) else { return };
    let v = ctx.cr(&victim);
    if v.inside.is_none() && dist(pos, v.pos) <= reach {
        let label = first_name(ctx.cr(id)).to_owned();
        damage_character(ctx, &victim, damage, Assailant { id: id.into(), label, character: Some(id.into()) }, mode);
    }
}

/// Drives a character whose intent is to fight.
pub fn run_fight(ctx: &mut Ctx, id: &str, target_id: &str, mode: FightMode, source: IntentSource) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let c = ctx.cr(id);
    let weapon = weapon_of(c, mode);

    if c.combat.mercy_until_tick > 0 {
        if tick >= c.combat.mercy_until_tick {
            let temper = c.profile.temper;
            let finish = temper > 0.8 && ctx.chance(0.35);
            resolve_mercy(ctx, id, !finish, None);
        }
        return;
    }

    let monster_pos =
        if mode == FightMode::Monster { get_monster(ctx.state, Some(target_id)).map(|m| m.pos) } else { None };
    let foe = if mode == FightMode::Monster { None } else { ctx.state.characters.get(target_id) };
    let Some(target_pos) = monster_pos.or(foe.map(|f| f.pos)) else {
        let outcome = if mode == FightMode::Monster { "The monster is destroyed." } else { "Your opponent is down." };
        end_fight(ctx, id, outcome.into());
        return;
    };
    if let Some(f) = foe
        && (f.inside.is_some() || f.combat.yielded)
    {
        let name = first_name(f).to_owned();
        if mode == FightMode::Lethal && c.has(Quirk::Murderous) && f.inside.is_some() {
            escaped(ctx, target_id, id);
        }
        end_fight(ctx, id, format!("{name} got away from you."));
        return;
    }

    if c.act.phase == ActPhase::Start {
        // Only the one who starts it is reported; hitting back is not "going for" anyone.
        let answering = foe.is_some_and(|f| f.intent.as_ref().is_some_and(|i| i.fights(id)));
        let report =
            foe.filter(|_| mode != FightMode::Monster && source != IntentSource::Reflex && !answering).map(|f| {
                let where_ = place_name_at(c.pos);
                if mode == FightMode::Brawl {
                    format!("{} went for {} with {} fists at {where_}.", first_name(c), first_name(f), their(c))
                } else {
                    format!("{} drew a weapon on {} at {where_}!", c.profile.name, f.profile.name)
                }
            });
        let a = ctx.c(id);
        a.act.phase = ActPhase::Perform;
        a.act.since_tick = tick;
        if let Some(text) = report {
            ctx.chronicle(ChronicleKind::Conflict, text, vec![id.into(), target_id.into()]);
            if mode == FightMode::Lethal {
                ctx.state.director.last_lethal_tick = tick;
            }
            ctx.emit(SimEvent::Emote { id: id.into(), emote: Emote::Anger });
        }
    }

    // A swing already in the air lands (or misses) before anything else happens.
    let c = ctx.c(id);
    if c.combat.hit_tick > 0 {
        face(c, target_pos);
        if tick >= c.combat.hit_tick {
            c.combat.hit_tick = 0;
            land_hit(ctx, id, weapon, mode);
        }
        return;
    }

    // Drink a potion rather than die.
    let cap = max_hp(c);
    if c.hp < cap * 0.38 && c.inventory.potion > 0 {
        c.inventory.potion -= 1;
        c.hp = cap.min(c.hp + POTION_HEAL);
        set_anim(c, AnimId::UseItem, true);
        c.combat.next_attack_tick = c.combat.next_attack_tick.max(tick + seconds_to_ticks(1.3));
        return;
    }
    // Those fighting on instinct know when they are beaten; born fighters hang on longer. A stand the
    // model ordered is held past that point, but nobody fights the dead to the last hit point: at half
    // the usual threshold even they break and run.
    let give_up = if c.has(Quirk::Fighter) { 0.1 } else { 0.34 - c.profile.bravery * 0.22 };
    let breaking_point = if source == IntentSource::Llm { give_up * LAST_RESORT } else { give_up };
    if mode == FightMode::Monster && c.hp < cap * breaking_point {
        start_flee(ctx, id, IntentSource::Reflex);
        return;
    }

    let d = dist(c.pos, target_pos);
    if d > weapon.range {
        if c.act.path.is_empty() || tick >= c.act.repath_tick {
            let route = route_to(&nav, c.pos, target_pos);
            c.act.repath_tick = tick + seconds_to_ticks(0.6);
            let Some(route) = route else {
                c.act.stuck += 1;
                if c.act.stuck > 8 {
                    end_fight(ctx, id, "You could not reach your opponent.".into());
                }
                return;
            };
            c.act.path = route;
        }
        let mut path = std::mem::take(&mut c.act.path);
        let speed = RUN_SPEED * c.stride();
        advance(c, &mut path, speed);
        c.act.path = path;
        set_anim(c, AnimId::Run, false);
        return;
    }

    c.act.path.clear();
    c.act.stuck = 0;
    face(c, target_pos);
    if tick >= c.combat.next_attack_tick {
        let from = c.pos;
        let anim = ctx.pick(weapon.anims);
        let c = ctx.c(id);
        set_anim(c, anim, true);
        let windup = clamp(anim.seconds() * 0.42, 0.3, 0.75);
        c.combat.hit_tick = tick + seconds_to_ticks(windup);
        c.combat.hit_target_id = Some(target_id.to_owned());
        c.combat.next_attack_tick = tick + seconds_to_ticks(weapon.interval);
        ctx.emit(SimEvent::Attack {
            attacker_id: id.into(),
            target_id: target_id.into(),
            ranged: projectile_of(weapon),
            from,
            to: target_pos,
        });
    } else if !is_one_shot_playing(c.anim, c.combat.next_attack_tick - seconds_to_ticks(weapon.interval), tick) {
        set_anim(c, weapon.stance, false);
    }
}

/// A murderer's blow from behind: several times what the weapon would do, and
/// then a fight to the finish. The opening is not announced; whether anyone
/// learns who did it depends on who was there to see.
pub fn sneak_attack(ctx: &mut Ctx, id: &str, target_id: &str) {
    let tick = ctx.tick();
    end_hunt(ctx, id);
    let target_pos = ctx.cr(target_id).pos;
    start_fight(ctx, id, target_id, FightMode::Lethal, IntentSource::Routine, None);
    let c = ctx.c(id);
    // Already under way: the usual "drew a weapon" report would give the game away.
    c.act.phase = ActPhase::Perform;
    c.act.since_tick = tick;
    face(c, target_pos);
    set_anim(c, AnimId::Attack1hStab, true);
    let weapon = weapon_of(c, FightMode::Lethal);
    c.combat.next_attack_tick = tick + seconds_to_ticks(weapon.interval);
    let from = c.pos;
    ctx.state.director.last_lethal_tick = tick;
    let damage = ctx.int_range(weapon.damage[0], weapon.damage[1]) * SNEAK_ATTACK_MULTIPLIER;
    ctx.emit(SimEvent::Attack {
        attacker_id: id.into(),
        target_id: target_id.into(),
        ranged: None,
        from,
        to: target_pos,
    });
    let label = first_name(ctx.cr(id)).to_owned();
    damage_character(
        ctx,
        target_id,
        damage,
        Assailant { id: id.into(), label, character: Some(id.into()) },
        FightMode::Lethal,
    );
    if ctx.alive(target_id) {
        ambushed(ctx, target_id, id);
    }
}
