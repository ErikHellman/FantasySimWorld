//! The valley's animals: the farm's herd and hens, the ducks on the shore, the birds about the village,
//! the owl in the glade, and the villagers' dogs and cats.
//!
//! Animals draw only from their own generator (`SimState.fauna.rng`) and take no part in `separate`, so
//! the villagers go on as they would without them except where one deals with an animal (tending,
//! petting, riding). The dead ignore them; they run from the dead.

use fsw_core::jsmath::js_hypot;
use fsw_core::math2d::{dist, facing_to};
use fsw_core::rng;
use fsw_core::time::{TICK_SECONDS, TICKS_PER_DAY, minutes_to_ticks, seconds_to_ticks};
use fsw_core::{Vec2, vec2};
use fsw_world::hex::{hex, hex_center};
use fsw_world::places::{find_place, get_place, homes};

use crate::data::animals::{AnimalAnim, DOG_SHARE, FOUNDING_ANIMALS, Haunt, PET_CHANCE, Species};
use crate::events::SimEvent;
use crate::nav::NavOverlay;
use crate::state::{ActPhase, Animal, ChronicleKind, Id, MemoryKind, MonsterPhase, SimState};
use crate::systems::movement::{advance, face, route_to};
use crate::util::{Ctx, adults, first_name};

/// The dead within this many paces send an animal running.
pub const FLEE_RADIUS: f64 = 10.0;
/// How long a new pet waits for its keeper to name it before the sim does.
const NAME_PATIENCE_MINUTES: f64 = 60.0;
/// A pet keeps this close to its keeper, and runs to catch up beyond `CATCH_UP`.
const HEEL: f64 = 3.0;
const CATCH_UP: f64 = 9.0;
/// Beyond this a pet stops following and goes home.
const FOLLOW_LIMIT: f64 = 70.0;
/// A walk shorter than this, in paces, is not worth a horse.
const RIDE_MIN: f64 = 70.0;
/// How far a villager setting out will go to fetch a free horse.
const MOUNT_REACH: f64 = 25.0;
/// Close enough to a horse to get on.
const MOUNT_DISTANCE: f64 = 1.6;
/// How long a horse waits where its rider got down before it goes home.
const WAIT_HOURS: f64 = 6.0;
const BODY_RADIUS: f64 = 0.45;
/// Longest a name may be, in UTF-16 units.
const NAME_CAP: usize = 16;
/// An animal moves every this many ticks, a stride at a time, each on a tick of its own: a viewer is sent a quarter
/// of the updates, and the client smooths over the gaps. A ridden horse moves with its rider every tick.
pub const STRIDE: i64 = 4;
const STRIDE_F: f64 = STRIDE as f64;

/// Animals sleep from dusk to dawn, the owl the other way about.
fn asleep_hours(hour: f64, nocturnal: bool) -> bool {
    let night = !(5.5..20.5).contains(&hour);
    night != nocturnal
}

fn roll(state: &mut SimState) -> f64 {
    rng::next_float(&mut state.fauna.rng)
}

fn roll_range(state: &mut SimState, min: f64, max: f64) -> f64 {
    rng::range(&mut state.fauna.rng, min, max)
}

fn roll_index(state: &mut SimState, len: usize) -> usize {
    rng::pick_index(&mut state.fauna.rng, len)
}

/// The middle of the meadow the herd grazes, south of the windmill.
pub fn pasture() -> Vec2 {
    hex_center(hex(3, 11))
}

/// The middle of where an animal keeps (for a bird about the village, one of the spots it visits).
fn haunt_centre(state: &mut SimState, haunt: Haunt) -> Vec2 {
    match haunt {
        Haunt::Pasture => pasture(),
        Haunt::Farmyard => get_place("farm").door,
        Haunt::Shore => get_place("docks").door,
        Haunt::Village | Haunt::Home => {
            const SPOTS: [&str; 3] = ["green", "market", "tavern"];
            get_place(SPOTS[roll_index(state, SPOTS.len())]).door
        }
        Haunt::Fields => hex_center(hex(2, 10)),
        Haunt::Glade => get_place("glade").door,
    }
}

/// A walkable spot within `radius` of `centre`, or the centre's nearest walkable ground.
fn spot_near(state: &mut SimState, nav: &NavOverlay, centre: Vec2, radius: f64) -> Option<Vec2> {
    let angle = roll_range(state, 0.0, std::f64::consts::TAU);
    let reach = radius * roll(state).sqrt();
    let (sin, cos) = (fsw_core::jsmath::sin(angle), fsw_core::jsmath::cos(angle));
    nav.nearest_walkable(vec2(centre.x + cos * reach, centre.z + sin * reach), 6)
}

fn new_animal_id(state: &mut SimState) -> Id {
    if state.fauna.next_id == 0 {
        state.fauna.next_id = 1;
    }
    let id = format!("a{}", state.fauna.next_id);
    state.fauna.next_id += 1;
    id
}

fn spawn(state: &mut SimState, species: Species, model: &str, pos: Vec2, facing: f64) -> Id {
    let id = new_animal_id(state);
    let anim = species.def().pause;
    state.animals.insert(
        id.clone(),
        Animal {
            id: id.clone(),
            species,
            model: model.to_owned(),
            name: None,
            owner: None,
            home_id: None,
            pos,
            facing,
            moving: false,
            anim,
            path: Vec::new(),
            flight: None,
            next_move_tick: 0,
            flee_until_tick: 0,
            rider: None,
            waiting_for: None,
            waiting_until_tick: 0,
            name_deadline_tick: 0,
            petted_tick: 0,
        },
    );
    id
}

fn name_taken(state: &SimState, name: &str) -> bool {
    let name = name.to_lowercase();
    state.animals.values().any(|a| a.name.as_deref().is_some_and(|n| n.to_lowercase() == name))
        || state.characters.values().any(|c| first_name(c).to_lowercase() == name)
}

/// A name from the species' list that no animal and no villager has, if any is left.
fn free_name(state: &mut SimState, species: Species) -> Option<String> {
    let free: Vec<&str> = species.def().names.iter().copied().filter(|n| !name_taken(state, n)).collect();
    (!free.is_empty()).then(|| free[roll_index(state, free.len())].to_owned())
}

/// The pets a villager keeps.
pub fn pets_of<'s>(state: &'s SimState, owner: &str) -> impl Iterator<Item = &'s Animal> {
    state.animals.values().filter(move |a| a.owner.as_deref() == Some(owner))
}

/// How a pet is spoken of before and after it has a name: "Biscuit", or "a new dog".
pub fn call(animal: &Animal) -> String {
    match &animal.name {
        Some(name) => name.clone(),
        None => animal.species.def().label.to_owned(),
    }
}

/// Gives a villager a dog or a cat, unnamed: the server may ask them what to call it.
fn adopt(ctx: &mut Ctx, owner: &str, species: Species, pos: Vec2) -> Id {
    let models = species.def().models;
    let model = models[roll_index(ctx.state, models.len())];
    let id = spawn(ctx.state, species, model, pos, 0.0);
    let deadline = ctx.tick() + minutes_to_ticks(NAME_PATIENCE_MINUTES);
    let home = ctx.cr(owner).home_id.clone();
    let animal = &mut ctx.state.animals[&id];
    animal.owner = Some(owner.to_owned());
    animal.home_id = Some(home);
    animal.name_deadline_tick = deadline;
    ctx.emit(SimEvent::PetAdopted { animal_id: id.clone(), owner_id: owner.to_owned() });
    id
}

/// The village gets its animals: once its founding is over, or, for a world saved before there were
/// any, on its first tick.
fn found(ctx: &mut Ctx) {
    let seed = ctx.state.seed;
    let fauna = &mut ctx.state.fauna;
    fauna.founded = true;
    fauna.rng = rng::hash_string(&format!("animals {seed}")).max(1);
    fauna.next_id = fauna.next_id.max(1);
    let nav = ctx.state.nav.clone();

    for (species, count) in FOUNDING_ANIMALS {
        let def = species.def();
        for _ in 0..count {
            let centre = haunt_centre(ctx.state, def.haunt);
            let Some(pos) = spot_near(ctx.state, &nav, centre, def.roam) else { continue };
            let model = def.models[roll_index(ctx.state, def.models.len())];
            let facing = roll_range(ctx.state, -std::f64::consts::PI, std::f64::consts::PI);
            let id = spawn(ctx.state, species, model, pos, facing);
            if def.rideable {
                let name = free_name(ctx.state, species);
                ctx.state.animals[&id].name = name;
            }
        }
    }

    // About half the homes keep a dog or a cat, which one of the grown-ups there calls their own.
    for home in homes() {
        let residents: Vec<Id> = adults(ctx.state).filter(|c| c.home_id == home.id).map(|c| c.id.clone()).collect();
        if residents.is_empty() || roll(ctx.state) >= PET_CHANCE {
            continue;
        }
        let species = if roll(ctx.state) < DOG_SHARE { Species::Dog } else { Species::Cat };
        let owner = residents[roll_index(ctx.state, residents.len())].clone();
        let Some(pos) = nav.nearest_walkable(home.door, 4) else { continue };
        adopt(ctx, &owner, species, pos);
    }

    let days = roll_range(ctx.state, 2.0, 4.0);
    ctx.state.fauna.next_stray_tick = ctx.tick() + (days * TICKS_PER_DAY as f64) as i64;
}

/// Now and then a stray dog or cat turns up on the west road and follows someone without one home.
fn strays(ctx: &mut Ctx) {
    let days = roll_range(ctx.state, 2.0, 4.0);
    ctx.state.fauna.next_stray_tick = ctx.tick() + (days * TICKS_PER_DAY as f64) as i64;
    let pets = ctx.state.animals.values().filter(|a| a.species.def().pet).count();
    if pets >= homes().len() {
        return;
    }
    let keepers: Vec<Id> = adults(ctx.state)
        .filter(|c| find_place(&c.home_id).is_some() && pets_of(ctx.state, &c.id).next().is_none())
        .map(|c| c.id.clone())
        .collect();
    if keepers.is_empty() {
        return;
    }
    let owner = keepers[roll_index(ctx.state, keepers.len())].clone();
    let species = if roll(ctx.state) < DOG_SHARE { Species::Dog } else { Species::Cat };
    let road = get_place("west_road").door;
    let Some(pos) = ctx.state.nav.nearest_walkable(road, 4) else { return };
    adopt(ctx, &owner, species, pos);
    let noun = species.def().noun;
    let who = first_name(ctx.cr(&owner)).to_owned();
    ctx.remember(
        &owner,
        format!("A stray {noun} came in off the west road and would not leave my side, so I took it in."),
        0.5,
        MemoryKind::Event,
        Vec::new(),
    );
    ctx.chronicle(
        ChronicleKind::World,
        format!("A stray {noun} came in off the west road and adopted {who}."),
        vec![owner],
    );
}

/// `Command::NameAnimal`: what a keeper calls their new pet.
pub fn name_pet(ctx: &mut Ctx, animal_id: &str, name: &str) {
    let Some(animal) = ctx.state.animals.get(animal_id) else { return };
    let (Some(owner), None) = (animal.owner.clone(), &animal.name) else { return };
    if !ctx.alive(&owner) {
        return;
    }
    let Some(name) = clean_name(name) else { return };
    if name_taken(ctx.state, &name) {
        return;
    }
    give_name(ctx, animal_id, &owner, name);
}

/// A usable pet's name: one or two words of letters (and the odd apostrophe or hyphen), capitalised.
pub fn clean_name(text: &str) -> Option<String> {
    let words: Vec<&str> = text.split_whitespace().take(2).collect();
    let joined = words.join(" ");
    let trimmed = joined.trim_matches(|c: char| !c.is_alphabetic());
    if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_alphabetic() || c == ' ' || c == '\'' || c == '-') {
        return None;
    }
    let mut chars = trimmed.chars();
    let first = chars.next()?;
    let name: String = first.to_uppercase().chain(chars).collect();
    (fsw_core::jsmath::utf16_len(&name) <= NAME_CAP).then_some(name)
}

fn give_name(ctx: &mut Ctx, animal_id: &str, owner: &str, name: String) {
    let animal = &mut ctx.state.animals[animal_id];
    animal.name = Some(name.clone());
    animal.name_deadline_tick = 0;
    let noun = animal.species.def().noun;
    let who = first_name(ctx.cr(owner)).to_owned();
    ctx.remember(owner, format!("My {noun} is called {name}."), 0.45, MemoryKind::Event, Vec::new());
    ctx.chronicle(ChronicleKind::World, format!("{who}'s {noun} is called {name}."), vec![owner.to_owned()]);
}

/// A pet whose keeper is gone goes to someone else in the house, or becomes a stray about the green.
fn rehome(ctx: &mut Ctx, animal_id: &str, home_id: Option<String>) {
    let heir = home_id.and_then(|home| {
        let housemates: Vec<&crate::state::Character> = adults(ctx.state).filter(|c| c.home_id == home).collect();
        housemates
            .iter()
            .find(|c| pets_of(ctx.state, &c.id).next().is_none())
            .or(housemates.first())
            .map(|c| c.id.clone())
    });
    let animal = &mut ctx.state.animals[animal_id];
    let dead_owner = animal.owner.take();
    animal.path.clear();
    let Some(heir) = heir else {
        animal.home_id = None;
        return;
    };
    animal.owner = Some(heir.clone());
    let pet = call(animal);
    let noun = animal.species.def().noun;
    let dead_name = dead_owner.map(|id| crate::util::name_of(ctx.state, &id)).unwrap_or_else(|| "someone".into());
    ctx.remember(
        &heir,
        format!("With {dead_name} gone, I have taken in {pet}, the {noun} {dead_name} kept."),
        0.5,
        MemoryKind::Event,
        Vec::new(),
    );
}

pub fn update_animals(ctx: &mut Ctx) {
    if !ctx.state.fauna.founded {
        if ctx.state.director.founding || ctx.state.characters.is_empty() {
            return;
        }
        found(ctx);
    }
    if ctx.tick() >= ctx.state.fauna.next_stray_tick {
        strays(ctx);
    }

    let nav = ctx.state.nav.clone();
    ride(ctx, &nav);
    let threats: Vec<Vec2> =
        ctx.state.monsters.values().filter(|m| m.phase != MonsterPhase::Dying).map(|m| m.pos).collect();
    let tick = ctx.tick();
    let ids: Vec<Id> = ctx.state.animals.keys().cloned().collect();
    for (index, id) in ids.iter().enumerate() {
        step_off(ctx.state, &nav, id, false);
        if (tick + index as i64) % STRIDE == 0 {
            step(ctx, &nav, &threats, id);
            step_off(ctx.state, &nav, id, true);
        }
    }
    nudge(ctx.state, &nav);
}

/// Riding: villagers who reached the horse they set out for get on; those setting out on a long walk with a free
/// horse a short way off go to it first; and a ridden horse goes where its rider goes until the ride ends, when it
/// waits there for them. Runs after `update_intents`, so a walk begun this tick is seen this tick.
fn ride(ctx: &mut Ctx, nav: &NavOverlay) {
    let tick = ctx.tick();
    let ridden: Vec<(Id, Id)> =
        ctx.state.animals.values().filter_map(|a| a.rider.clone().map(|r| (a.id.clone(), r))).collect();
    for (horse, rider) in ridden {
        // A rider keeps on from one errand to the next only while the next is a long one too.
        let riding = ctx.state.characters.get(&rider).is_some_and(|c| {
            c.mount.as_deref() == Some(horse.as_str())
                && c.inside.is_none()
                && c.act.phase == ActPhase::Travel
                && !c.act.path.is_empty()
                && (c.act.since_tick != tick || path_length(c.pos, &c.act.path) >= RIDE_MIN)
        });
        if riding {
            let c = &ctx.state.characters[&rider];
            let (pos, facing, moving) = (c.pos, c.facing, c.moving);
            let animal = ctx.state.animals.get_mut(&horse).expect("horse");
            animal.pos = pos;
            animal.facing = facing;
            animal.moving = moving;
            animal.anim = AnimalAnim::Ridden;
        } else {
            dismount(ctx, &horse, &rider);
        }
    }

    for id in crate::util::living_ids(ctx.state) {
        let c = &ctx.state.characters[&id];
        if let Some(horse) = c.mounting.clone() {
            let walking = c.act.phase == ActPhase::Travel && !c.act.path.is_empty() && c.inside.is_none();
            let held =
                ctx.state.animals.get(&horse).filter(|a| a.rider.is_none() && a.waiting_for.as_deref() == Some(&id));
            match held {
                Some(a) if walking && dist(a.pos, c.pos) < MOUNT_DISTANCE => mount(ctx, &id, &horse),
                Some(_) if walking => {}
                _ => {
                    ctx.c(&id).mounting = None;
                    if let Some(a) = ctx.state.animals.get_mut(&horse)
                        && a.waiting_for.as_deref() == Some(id.as_str())
                        && a.rider.is_none()
                    {
                        a.waiting_for = None;
                    }
                }
            }
            continue;
        }
        // Only a walk begun this tick, by a grown villager out of doors, long enough to be worth a horse.
        if c.mount.is_some()
            || c.is_child()
            || c.inside.is_some()
            || c.act.phase != ActPhase::Travel
            || c.act.since_tick != tick
        {
            continue;
        }
        let Some(dest) = c.act.dest else { continue };
        let walk = path_length(c.pos, &c.act.path);
        if walk < RIDE_MIN {
            continue;
        }
        let from = c.pos;
        let free = ctx
            .state
            .animals
            .values()
            .filter(|a| {
                a.species.def().rideable
                    && a.rider.is_none()
                    && a.waiting_for.as_deref().is_none_or(|w| w == id)
                    && tick >= a.flee_until_tick
                    && dist(a.pos, from) < MOUNT_REACH
            })
            .min_by(|a, b| dist(a.pos, from).total_cmp(&dist(b.pos, from)))
            .map(|a| (a.id.clone(), a.pos));
        let Some((horse, at)) = free else { continue };
        let (Some(to_horse), Some(onward)) = (route_to(nav, from, at), route_to(nav, at, dest)) else { continue };
        // Not worth it when fetching the horse is most of the way.
        if path_length(from, &to_horse) > walk * 0.35 {
            continue;
        }
        let c = ctx.c(&id);
        c.act.path = [to_horse, onward].concat();
        c.mounting = Some(horse.clone());
        let animal = ctx.state.animals.get_mut(&horse).expect("horse");
        animal.waiting_for = Some(id.clone());
        animal.waiting_until_tick = tick + minutes_to_ticks(10.0);
        animal.path.clear();
    }
}

fn path_length(from: Vec2, path: &[Vec2]) -> f64 {
    let mut length = 0.0;
    let mut at = from;
    for &point in path {
        length += dist(at, point);
        at = point;
    }
    length
}

fn mount(ctx: &mut Ctx, rider: &str, horse: &str) {
    let c = ctx.c(rider);
    c.mounting = None;
    c.mount = Some(horse.to_owned());
    let pos = c.pos;
    let animal = ctx.state.animals.get_mut(horse).expect("horse");
    animal.rider = Some(rider.to_owned());
    animal.waiting_for = None;
    animal.path.clear();
    animal.pos = pos;
    animal.anim = AnimalAnim::Ridden;
}

/// The ride is over: the rider gets down and the horse waits there for them, a while.
fn dismount(ctx: &mut Ctx, horse: &str, rider: &str) {
    let tick = ctx.tick();
    let alive = ctx.alive(rider);
    if alive && ctx.cr(rider).mount.as_deref() == Some(horse) {
        ctx.c(rider).mount = None;
    }
    let animal = ctx.state.animals.get_mut(horse).expect("horse");
    animal.rider = None;
    animal.path.clear();
    animal.moving = false;
    animal.anim = animal.species.def().pause;
    if alive {
        animal.waiting_for = Some(rider.to_owned());
        animal.waiting_until_tick = tick + minutes_to_ticks(WAIT_HOURS * 60.0);
    }
}

/// Where to stand to tend one of the farm's animals, and which: one of those that give something, by `pick` (0..1).
pub fn tending_spot(state: &SimState, nav: &NavOverlay, pick: f64) -> Option<Vec2> {
    let herd: Vec<&Animal> = state
        .animals
        .values()
        .filter(|a| a.species.def().yields.is_some() && a.rider.is_none() && a.owner.is_none())
        .collect();
    if herd.is_empty() {
        return None;
    }
    let animal = herd[((pick * herd.len() as f64) as usize).min(herd.len() - 1)];
    nav.nearest_walkable(vec2(animal.pos.x + 1.3, animal.pos.z), 3)
}

/// Keeps the animal nearest `pos` (within a few paces) where it is until `until`, while someone sees to it; where
/// it stands.
pub fn hold_still(state: &mut SimState, pos: Vec2, until: i64, pets: bool) -> Option<Vec2> {
    let animal = state
        .animals
        .values_mut()
        .filter(|a| a.rider.is_none() && (pets || a.owner.is_none()) && dist(a.pos, pos) < 4.0)
        .min_by(|a, b| dist(a.pos, pos).total_cmp(&dist(b.pos, pos)))?;
    animal.path.clear();
    animal.flight = None;
    animal.next_move_tick = animal.next_move_tick.max(until);
    animal.anim = animal.species.def().pause;
    Some(animal.pos)
}

/// A pet a villager might stop to make a fuss of: their own, near and not fussed over lately; for a child, any
/// dog or cat near.
pub fn pet_to_fuss(state: &SimState, id: &str) -> Option<Id> {
    let c = state.characters.get(id)?;
    if c.inside.is_some() || c.mount.is_some() {
        return None;
    }
    state
        .animals
        .values()
        .filter(|a| {
            a.species.def().pet
                && a.rider.is_none()
                && (a.owner.as_deref() == Some(id) || c.is_child())
                && dist(a.pos, c.pos) < 8.0
                && state.tick - a.petted_tick > minutes_to_ticks(180.0)
        })
        .min_by(|a, b| dist(a.pos, c.pos).total_cmp(&dist(b.pos, c.pos)))
        .map(|a| a.id.clone())
}

/// Off blocked ground at once: where the village has built on an animal (which then finds a new way), or where a
/// stride cut a corner (which then goes on to where its path was taking it).
///
/// The open ground it steps onto is the middle of the nearest open cell, which can be off its path: from there the
/// straight line to the next point may cut the same corner again, and an animal that kept that path stepped onto
/// blocked ground and back off it every stride without end. So it finds its way afresh from where it stands, and
/// when that way is the one it had (the search's straight cut misses the corner too), it gives up on going there.
fn step_off(state: &mut SimState, nav: &NavOverlay, id: &str, keep_path: bool) {
    let animal = state.animals.get_mut(id).expect("animal");
    if animal.rider.is_some() || animal.flight.is_some() || nav.is_walkable(animal.pos.x, animal.pos.z) {
        return;
    }
    if let Some(open) = nav.nearest_walkable(animal.pos, 3) {
        animal.pos = open;
    }
    let goal = animal.path.last().copied().filter(|_| keep_path);
    let fresh = goal.and_then(|goal| route_to(nav, animal.pos, goal)).unwrap_or_default();
    animal.path = if fresh == animal.path { Vec::new() } else { fresh };
}

/// Walks a stride along the animal's path; true once the path is done. A stride that lands on blocked ground (built
/// over since the path was found, or a corner the path cuts) is put right by `step_off` straight after, which finds
/// the rest of the way again.
fn walk(animal: &mut Animal, speed: f64) -> bool {
    let mut path = std::mem::take(&mut animal.path);
    let done = advance(animal, &mut path, speed * STRIDE_F);
    animal.path = path;
    done
}

fn step(ctx: &mut Ctx, nav: &NavOverlay, threats: &[Vec2], id: &str) {
    let tick = ctx.tick();
    let Some(animal) = ctx.state.animals.get_mut(id) else { return };
    // A ridden horse goes where its rider takes it (`systems::intents`).
    if animal.rider.is_some() {
        return;
    }
    animal.moving = false;
    let def = animal.species.def();

    // A pet whose keeper has died goes to someone else.
    if let Some(owner) = animal.owner.clone()
        && !ctx.state.characters.contains_key(&owner)
    {
        let home = ctx.state.animals[id].home_id.clone();
        rehome(ctx, id, home);
    }
    let animal = &ctx.state.animals[id];
    if animal.owner.is_some() && animal.name.is_none() && tick >= animal.name_deadline_tick {
        let owner = animal.owner.clone().unwrap_or_default();
        let name = free_name(ctx.state, def.species).unwrap_or_else(|| def.names[0].to_owned());
        give_name(ctx, id, &owner, name);
    }

    // It keeps an eye out for the dead.
    {
        let pos = ctx.state.animals[id].pos;
        let near = threats
            .iter()
            .copied()
            .filter(|t| dist(*t, pos) < FLEE_RADIUS)
            .min_by(|a, b| dist(*a, pos).total_cmp(&dist(*b, pos)));
        if let Some(threat) = near {
            run_from(ctx.state, nav, id, threat);
        }
    }
    let animal = &ctx.state.animals[id];
    if tick < animal.flee_until_tick {
        let speed = def.run;
        let animal = ctx.state.animals.get_mut(id).expect("animal");
        if def.flies {
            fly(animal, speed);
        } else {
            walk(animal, speed);
            animal.anim = if animal.moving { AnimalAnim::Run } else { def.pause };
        }
        return;
    }

    let asleep = asleep_hours(ctx.time.hour, def.nocturnal);
    if def.haunt == Haunt::Home && animal.owner.is_some() {
        keep_to_owner(ctx, nav, id, asleep);
    } else if def.flies {
        bird(ctx, nav, id, asleep);
    } else {
        graze(ctx, nav, id, asleep);
    }
}

/// Runs straight away from one of the dead.
fn run_from(state: &mut SimState, nav: &NavOverlay, id: &str, threat: Vec2) {
    let tick = state.tick;
    let animal = &state.animals[id];
    let (mut dx, mut dz) = (animal.pos.x - threat.x, animal.pos.z - threat.z);
    // Right on top of it, it bolts the way it faces.
    if js_hypot(dx, dz) < 0.1 {
        (dx, dz) = (fsw_core::jsmath::sin(animal.facing), fsw_core::jsmath::cos(animal.facing));
    }
    let d = js_hypot(dx, dz);
    let away = vec2(animal.pos.x + dx / d * 14.0, animal.pos.z + dz / d * 14.0);
    let flies = animal.species.def().flies;
    let from = animal.pos;
    let route = if flies { None } else { nav.nearest_walkable(away, 6).and_then(|goal| route_to(nav, from, goal)) };
    let animal = state.animals.get_mut(id).expect("animal");
    animal.flee_until_tick = tick + seconds_to_ticks(5.0);
    animal.waiting_for = None;
    if flies {
        animal.flight = Some(away);
    } else if let Some(path) = route {
        animal.path = path;
    }
}

/// One tick of a bird in the air, straight to where it lands.
fn fly(animal: &mut Animal, speed: f64) {
    let Some(dest) = animal.flight else {
        animal.anim = animal.species.def().pause;
        return;
    };
    face(animal, dest);
    animal.facing = facing_to(animal.pos, dest);
    let pos = animal.pos;
    let d = dist(pos, dest);
    let step = speed * TICK_SECONDS * STRIDE_F;
    if d <= step {
        animal.pos = dest;
        animal.flight = None;
        animal.anim = animal.species.def().pause;
    } else {
        animal.pos = vec2(pos.x + (dest.x - pos.x) / d * step, pos.z + (dest.z - pos.z) / d * step);
        animal.moving = true;
        animal.anim = AnimalAnim::Fly;
    }
}

/// The herd, the hens, the ducks and the owl: wander their haunt, stop to graze, and sleep huddled near
/// its middle at night. One that has strayed (a horse left where its rider got down, one that ran from
/// the dead) finds its way back the same way.
fn graze(ctx: &mut Ctx, nav: &NavOverlay, id: &str, asleep: bool) {
    let tick = ctx.tick();
    let animal = ctx.state.animals.get_mut(id).expect("animal");
    let def = animal.species.def();

    // A horse waiting for its rider stays put, grazing, until they come back or it tires of waiting.
    if animal.waiting_for.is_some() {
        let rider_gone = animal.waiting_for.as_deref().is_none_or(|r| !ctx.state.characters.contains_key(r));
        let animal = ctx.state.animals.get_mut(id).expect("animal");
        if !rider_gone && tick < animal.waiting_until_tick && !asleep {
            animal.path.clear();
            animal.anim = def.pause;
            return;
        }
        animal.waiting_for = None;
        animal.waiting_until_tick = 0;
        animal.next_move_tick = 0;
    }

    let animal = ctx.state.animals.get_mut(id).expect("animal");
    if !animal.path.is_empty() {
        let done = walk(animal, def.walk);
        animal.anim = AnimalAnim::Walk;
        if done {
            let pause = if asleep { 60.0 } else { 4.0 + roll(ctx.state) * 12.0 };
            let animal = ctx.state.animals.get_mut(id).expect("animal");
            animal.next_move_tick = tick + seconds_to_ticks(pause);
            animal.anim = if asleep { AnimalAnim::Sleep } else { def.pause };
        }
        return;
    }
    if tick < animal.next_move_tick {
        animal.anim = if asleep && animal.anim != AnimalAnim::Walk { AnimalAnim::Sleep } else { def.pause };
        return;
    }
    // The owl keeps its perch: it only looks about.
    if def.nocturnal {
        let turn = roll_range(ctx.state, -1.2, 1.2);
        let wait = roll_range(ctx.state, 6.0, 20.0);
        let animal = ctx.state.animals.get_mut(id).expect("animal");
        animal.facing += if asleep { 0.0 } else { turn };
        animal.anim = if asleep { AnimalAnim::Sleep } else { AnimalAnim::Idle };
        animal.next_move_tick = tick + seconds_to_ticks(wait);
        return;
    }
    let centre = haunt_centre(ctx.state, def.haunt);
    let pos = ctx.state.animals[id].pos;
    let reach = if asleep { 3.0 } else { def.roam };
    // Asleep, one already near the middle stays where it is.
    if asleep && dist(pos, centre) < def.roam * 0.6 {
        let animal = ctx.state.animals.get_mut(id).expect("animal");
        animal.anim = AnimalAnim::Sleep;
        animal.next_move_tick = tick + seconds_to_ticks(60.0);
        return;
    }
    let goal = spot_near(ctx.state, nav, centre, reach);
    let path = goal.and_then(|goal| route_to(nav, pos, goal)).unwrap_or_default();
    let animal = ctx.state.animals.get_mut(id).expect("animal");
    animal.next_move_tick = tick + seconds_to_ticks(3.0);
    animal.path = path;
}

/// A bird: pecks about, hops a little, and now and then flies off to another spot in its haunt.
fn bird(ctx: &mut Ctx, nav: &NavOverlay, id: &str, asleep: bool) {
    let tick = ctx.tick();
    let animal = ctx.state.animals.get_mut(id).expect("animal");
    let def = animal.species.def();
    if animal.flight.is_some() {
        fly(animal, def.run);
        if animal.flight.is_none() {
            let pause = 3.0 + roll(ctx.state) * 7.0;
            ctx.state.animals[id].next_move_tick = tick + seconds_to_ticks(pause);
        }
        return;
    }
    if !animal.path.is_empty() {
        walk(animal, def.walk);
        animal.anim = AnimalAnim::Walk;
        return;
    }
    if tick < animal.next_move_tick {
        animal.anim = if asleep { AnimalAnim::Sleep } else { def.pause };
        return;
    }
    if asleep {
        animal.anim = AnimalAnim::Sleep;
        animal.next_move_tick = tick + seconds_to_ticks(60.0);
        return;
    }
    let pos = animal.pos;
    if roll(ctx.state) < 0.3 {
        let centre = haunt_centre(ctx.state, def.haunt);
        if let Some(spot) = spot_near(ctx.state, nav, centre, def.roam) {
            ctx.state.animals[id].flight = Some(spot);
        }
    } else if let Some(spot) = spot_near(ctx.state, nav, pos, 2.5) {
        let path = route_to(nav, pos, spot).unwrap_or_default();
        ctx.state.animals[id].path = path;
    }
    let wait = 2.0 + roll(ctx.state) * 4.0;
    ctx.state.animals[id].next_move_tick = tick + seconds_to_ticks(wait);
}

/// A dog or a cat: at its keeper's heels while they are out, by the door of wherever they are while
/// they are in, and asleep there at night.
fn keep_to_owner(ctx: &mut Ctx, nav: &NavOverlay, id: &str, asleep: bool) {
    let tick = ctx.tick();
    let animal = &ctx.state.animals[id];
    let def = animal.species.def();
    let owner = animal.owner.as_deref().and_then(|o| ctx.state.characters.get(o));
    let Some(owner) = owner else { return };
    let pos = animal.pos;
    let near = owner.inside.is_none() && dist(owner.pos, pos) < FOLLOW_LIMIT;
    let (target, close) = if near {
        (owner.pos, HEEL)
    } else {
        let door = owner
            .inside
            .as_deref()
            .and_then(find_place)
            .or_else(|| find_place(&owner.home_id))
            .map(|place| place.door)
            .unwrap_or(owner.pos);
        (door, 1.5)
    };
    let owner_moving = owner.moving;
    let d = dist(pos, target);
    let animal = ctx.state.animals.get_mut(id).expect("animal");

    if d <= close {
        animal.path.clear();
        if near {
            face(animal, target);
        }
        animal.anim = if asleep && !owner_moving { AnimalAnim::Sleep } else { def.pause };
        return;
    }
    // Once a second, so a keeper out of reach does not cost a search every tick.
    if tick >= animal.next_move_tick {
        let path = route_to(nav, pos, target).unwrap_or_default();
        animal.path = path;
        animal.next_move_tick = tick + seconds_to_ticks(1.0);
    }
    // The path ends on its keeper; it stops short of that, at their heels, on the next tick.
    let speed = if d > CATCH_UP { def.run } else { def.walk };
    walk(animal, speed);
    animal.anim = if speed > def.walk { AnimalAnim::Run } else { AnimalAnim::Walk };
}

/// Keeps animals from standing in each other and in villagers, each on its own tick. Only animals move; the
/// villagers' own paths are never pushed, so a world goes on the same with its animals as without.
///
/// Only an animal standing still is pushed. One on the move brushes past whoever is in its way, as villagers do,
/// and the ones standing make room for it: pushing walkers too held two of them meeting head on in a lane one cell
/// wide (or one walking into a villager who stood there) in place for good, their legs going and neither getting
/// anywhere.
fn nudge(state: &mut SimState, nav: &NavOverlay) {
    let tick = state.tick;
    let mut bodies: Vec<Vec2> = state.characters.values().filter(|c| c.inside.is_none()).map(|c| c.pos).collect();
    let first_animal = bodies.len();
    bodies.extend(state.animals.values().map(|a| a.pos));
    for (index, animal) in state.animals.values_mut().enumerate() {
        let def = animal.species.def();
        if (tick + index as i64) % STRIDE != 0
            || animal.moving
            || animal.rider.is_some()
            || animal.flight.is_some()
            || def.flies
        {
            continue;
        }
        let me = first_animal + index;
        let (mut push_x, mut push_z) = (0.0, 0.0);
        for (other, &pos) in bodies.iter().enumerate() {
            if other == me {
                continue;
            }
            let (dx, dz) = (animal.pos.x - pos.x, animal.pos.z - pos.z);
            let d = js_hypot(dx, dz);
            if !(1e-4..BODY_RADIUS * 2.0).contains(&d) {
                continue;
            }
            let overlap = BODY_RADIUS * 2.0 - d;
            push_x += dx / d * overlap * 0.3;
            push_z += dz / d * overlap * 0.3;
        }
        if push_x == 0.0 && push_z == 0.0 {
            continue;
        }
        let (x, z) = (animal.pos.x + push_x, animal.pos.z + push_z);
        if nav.is_walkable(x, z) {
            animal.pos = vec2(x, z);
            bodies[me] = animal.pos;
        }
    }
}
