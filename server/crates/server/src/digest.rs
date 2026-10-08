//! The HUD's view of the world, rendered on the server because the client has
//! no sim helpers: the roster, the status line, the inspector's
//! `CharacterDetail` (port of `tools/ref-server/digest.ts` at the `ts-sim-reference` tag) and a
//! site's `SiteDetail`.

use fsw_core::jsmath::js_round;
use fsw_core::time::{format_clock, game_time, part_of_day};
use fsw_core::vec2;
use fsw_sim::data::activities::activity;
use fsw_sim::describe::describe_activity;
use fsw_sim::state::{
    ActPhase, Character, HousePhase, IntentAction, MemoryKind as SimMemoryKind, MonsterPhase, PlanSource, ProjectKind,
    PropStage, RelationLabel as SimLabel, Resource, SimState, WorldObject as SimWorldObject,
};
use fsw_sim::systems::combat::max_hp;
use fsw_sim::systems::fire::{built_fraction, is_homeless, site_name};
use fsw_sim::systems::relationships::rung_of;
use fsw_sim::systems::routine::block_at;
use fsw_sim::systems::world::{beds_of, object_name, project_label};
use fsw_sim::util::{first_name, living, name_of};
use fsw_world::places::{ActivityId, PlaceKind, find_place, nearest_place};

use crate::api::wire::{
    AmbientCounts, CharacterDetail, ChronicleEntry, Founding, Goods, Inventory, LlmStatus, Memory, MemoryKind, Mood,
    Needs, Person, PlanRow, RelationLabel, RelationRow, RosterEntry, SiteDetail, SiteKind, SiteNeed, SiteProject,
    Status,
};
use crate::encode::{chronicle_entry, object_facts};

/// How many chronicle entries a snapshot carries; older ones come from `GET /api/chronicle`.
pub const CHRONICLE_TAIL: usize = 120;
/// How many memories the inspector shows, newest first.
pub const DETAIL_MEMORIES: usize = 40;
/// How many chronicle lines a site's page shows, newest first.
pub const SITE_HISTORY: usize = 20;
/// A villager this close to a place's origin counts as being there (the same reach as `place_name_at`).
const PRESENT_REACH: f64 = 16.0;

pub fn mood_of(c: &Character) -> Mood {
    if matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::Fight { .. } | IntentAction::Flee)) {
        Mood::Fight
    } else if c.asleep {
        Mood::Sleep
    } else if c.convo_id.is_some() {
        Mood::Talk
    } else if c.hp < max_hp(c) * 0.5 {
        Mood::Hurt
    } else {
        Mood::Ok
    }
}

pub fn roster_of(state: &SimState) -> Vec<RosterEntry> {
    living(state)
        .map(|c| RosterEntry {
            id: c.id.clone(),
            name: c.profile.name.clone(),
            role: c.profile.occupation.clone(),
            status: describe_activity(state, c),
            hp: c.hp / max_hp(c),
            mood: mood_of(c),
            partner: c.partner_id.is_some(),
            child: c.is_child(),
        })
        .collect()
}

pub fn is_alarm(state: &SimState) -> bool {
    state.tick < state.director.alarm_until_tick
}

/// Monsters still in the fight (a dying one is not counted).
pub fn monster_count(state: &SimState) -> u32 {
    state.monsters.values().filter(|m| m.phase != MonsterPhase::Dying).count() as u32
}

/// Villagers performing an activity (not on their way to it).
fn doing(state: &SimState, id: ActivityId) -> u32 {
    living(state)
        .filter(|c| c.intent.as_ref().is_some_and(|i| i.is_activity(id)) && c.act.phase == ActPhase::Perform)
        .count() as u32
}

pub fn ambient_of(state: &SimState) -> AmbientCounts {
    AmbientCounts {
        awake: living(state).filter(|c| !c.asleep).count() as u32,
        eating: doing(state, ActivityId::Eat),
        drinking: doing(state, ActivityId::Drink),
        smithing: doing(state, ActivityId::Smith),
        relaxing: doing(state, ActivityId::Relax),
    }
}

pub struct StatusExtras {
    pub llm: LlmStatus,
    pub viewers: u32,
    pub speed: u8,
    pub throttled: bool,
}

pub fn status_of(state: &SimState, extras: StatusExtras) -> Status {
    Status {
        llm: extras.llm,
        viewers: extras.viewers,
        speed: extras.speed,
        throttled: extras.throttled,
        population: state.order.len() as u32,
        monsters: monster_count(state),
        alarm: is_alarm(state),
        part: part_of_day(game_time(state.tick).hour).into(),
        ambient: ambient_of(state),
        founding: state
            .director
            .founding
            .then_some(Founding { settled: state.order.len() as u32, total: state.director.population_target as u32 }),
    }
}

fn label(label: SimLabel) -> RelationLabel {
    match label {
        SimLabel::Stranger => RelationLabel::Stranger,
        SimLabel::Acquaintance => RelationLabel::Acquaintance,
        SimLabel::Friend => RelationLabel::Friend,
        SimLabel::CloseFriend => RelationLabel::CloseFriend,
        SimLabel::Lover => RelationLabel::Lover,
        SimLabel::Rival => RelationLabel::Rival,
        SimLabel::Enemy => RelationLabel::Enemy,
        SimLabel::Family => RelationLabel::Family,
        SimLabel::Spouse => RelationLabel::Spouse,
    }
}

/// Everyone alive the character has a record of, and the dead they felt
/// strongly about, strongest feelings first. The rung is rendered with a space ("blood feud").
fn relations_of(state: &SimState, c: &Character) -> Vec<RelationRow> {
    let mut rows: Vec<RelationRow> = c
        .relationships
        .iter()
        .map(|(id, r)| {
            let other = state.characters.get(id);
            let name = match (other, state.dead.get(id)) {
                (Some(o), _) => o.profile.name.clone(),
                (None, Some(dead)) => format!("{} †", dead.name),
                _ => String::new(),
            };
            RelationRow {
                id: id.clone(),
                name,
                label: label(r.label),
                affinity: r.affinity,
                attraction: r.attraction,
                grievance: r.grievance,
                rung: rung_of(r).as_str().replacen('_', " ", 1),
                summary: r.summary.clone(),
                alive: other.is_some(),
            }
        })
        .filter(|row| !row.name.is_empty() && (row.alive || row.affinity.abs() >= 25.0))
        .collect();
    let weight = |row: &RelationRow| row.affinity.abs() + row.grievance + row.attraction;
    // Stable, strongest first (as Array.prototype.sort with `b - a`).
    rows.sort_by(|a, b| weight(b).partial_cmp(&weight(a)).unwrap_or(std::cmp::Ordering::Equal));
    rows
}

fn memory_kind(kind: SimMemoryKind) -> MemoryKind {
    match kind {
        SimMemoryKind::Event => MemoryKind::Event,
        SimMemoryKind::Talk => MemoryKind::Talk,
        SimMemoryKind::Thought => MemoryKind::Thought,
        SimMemoryKind::Reflection => MemoryKind::Reflection,
    }
}

/// The inspector for one living character.
pub fn detail_of(state: &SimState, id: &str) -> Option<CharacterDetail> {
    let c = state.characters.get(id)?;
    let current = block_at(&c.plan, game_time(state.tick).hour);
    Some(CharacterDetail {
        id: c.id.clone(),
        name: c.profile.name.clone(),
        pronouns: c.profile.pronouns.as_str().into(),
        age: c.profile.age.max(0) as u32,
        occupation: c.profile.occupation.clone(),
        class_name: c.class_id.def().label.into(),
        child: c.is_child(),
        backstory: c.profile.backstory.clone(),
        traits: c.profile.traits.clone(),
        goals: c.profile.goals.clone(),
        status: describe_activity(state, c),
        thought: c.thought.clone(),
        summary: c.summary.clone(),
        hp: c.hp,
        max_hp: max_hp(c),
        needs: Needs { hunger: c.needs.hunger, energy: c.needs.energy, social: c.needs.social },
        inventory: Inventory {
            food: c.inventory.food.max(0) as u32,
            potion: c.inventory.potion.max(0) as u32,
            gift: c.inventory.gift.max(0) as u32,
            trinket: c.inventory.trinket.max(0) as u32,
            relic: c.inventory.relic.max(0) as u32,
        },
        carried: Goods {
            wood: c.carried.get(Resource::Wood).max(0) as u32,
            stone: c.carried.get(Resource::Stone).max(0) as u32,
            ore: c.carried.get(Resource::Ore).max(0) as u32,
        },
        home: find_place(&c.home_id).map_or(c.home_id.clone(), |place| place.name.into()),
        homeless: is_homeless(state, c),
        spouse: c.spouse_id.as_deref().map(|spouse| name_of(state, spouse)),
        gold: c.gold.max(0) as u32,
        kills: c.kills.max(0) as u32,
        plan_source: match c.plan_source {
            PlanSource::Routine => "routine".into(),
            PlanSource::Llm => "llm".into(),
        },
        plan: c
            .plan
            .iter()
            .enumerate()
            .map(|(index, block)| PlanRow {
                time: format_clock(block.hour * 60.0),
                text: match block.note.as_deref().filter(|n| !n.is_empty()) {
                    Some(note) => format!("{} — {note}", activity(block.activity).label),
                    None => activity(block.activity).label.into(),
                },
                current: index as i64 == current,
            })
            .collect(),
        relations: relations_of(state, c),
        memories: c
            .memories
            .iter()
            .rev()
            .take(DETAIL_MEMORIES)
            .map(|m| Memory {
                tick: m.tick.max(0) as u64,
                day: m.day.max(0) as u32,
                text: m.text.clone(),
                importance: m.importance.clamp(0, 255) as u8,
                kind: memory_kind(m.kind),
                subjects: m.subjects.clone(),
            })
            .collect(),
    })
}

/// Whether an id names a site (a building or something the village built) rather than a villager.
pub fn is_site_id(id: &str) -> bool {
    id.starts_with("b:") || id.starts_with("n:")
}

/// The page the watcher of `id` receives, serialised: a villager's detail or a site's.
pub fn detail_json(state: &SimState, id: &str) -> Option<String> {
    if is_site_id(id) {
        site_of(state, id).map(|d| serde_json::to_string(&d).expect("site details serialise"))
    } else {
        detail_of(state, id).map(|d| serde_json::to_string(&d).expect("details serialise"))
    }
}

fn person(c: &Character) -> Person {
    Person { id: c.id.clone(), name: first_name(c).to_owned() }
}

/// The page for a building (`b:<place>`) or something the village built (`n:<n>`); `None` for anything
/// else, including a built thing that is gone.
pub fn site_of(state: &SimState, id: &str) -> Option<SiteDetail> {
    let object = state.objects.get(id);
    let (kind, place) = if let Some(place_id) = id.strip_prefix("b:") {
        (SiteKind::Building, find_place(place_id).filter(|place| place.has_building)?)
    } else {
        let Some(SimWorldObject::Built { spec, stage, .. }) = object else { return None };
        if *stage == PropStage::Gone {
            return None;
        }
        (SiteKind::Built, nearest_place(vec2(spec.x, spec.z), f64::MAX)?)
    };
    let building = kind == SiteKind::Building;
    // A home's fire and ruin live in `houses`; everything else done to a building is in `objects`.
    let (stage, since_tick, progress, fire) = match (object, state.houses.get(&place.id).filter(|_| building)) {
        (Some(object), _) => object_facts(object),
        (None, Some(house)) => match house.phase {
            HousePhase::Burning => ("burning".to_owned(), house.since_tick, 0, round5(house.fire)),
            HousePhase::Ruined => {
                ("ruin".to_owned(), house.since_tick, (built_fraction(house) * 100.0).floor() as u8, 0)
            }
        },
        (None, None) => ("whole".to_owned(), 0, 0, 0),
    };
    let home = building && place.kind == PlaceKind::Home;
    let mut present: Vec<Person> = if building {
        living(state)
            .filter(|c| {
                c.inside.as_deref() == Some(place.id.as_str())
                    || nearest_place(c.pos, PRESENT_REACH).is_some_and(|near| near.id == place.id)
            })
            .map(person)
            .collect()
    } else {
        Vec::new()
    };
    present.sort_by(|a, b| a.name.cmp(&b.name));
    let project = state
        .projects
        .iter()
        .find(|p| {
            p.target == id
                || (building
                    && p.kind == ProjectKind::Construct
                    && p.target.split('/').next() == Some(place.id.as_str()))
        })
        .map(|p| SiteProject {
            label: project_label(state, p),
            helpers: p.helpers.iter().filter_map(|h| state.characters.get(h)).map(person).collect(),
            needs: p
                .cost
                .iter()
                .map(|(resource, needed)| SiteNeed {
                    resource: resource.as_str().to_owned(),
                    delivered: p.delivered.get(resource).clamp(0, needed) as u32,
                    needed: needed.max(0) as u32,
                })
                .collect(),
            progress: if p.need > 0.0 { ((p.work / p.need).clamp(0.0, 1.0) * 100.0).floor() as u8 } else { 0 },
        });
    let stockpile =
        state.stockpiles.get(&place.id).filter(|_| building).filter(|store| !store.is_empty()).map(|store| Goods {
            wood: store.get(Resource::Wood).max(0) as u32,
            stone: store.get(Resource::Stone).max(0) as u32,
            ore: store.get(Resource::Ore).max(0) as u32,
        });
    Some(SiteDetail {
        id: id.to_owned(),
        kind,
        name: match object {
            Some(object) if !building => object_name(id, object),
            _ => site_name(id),
        },
        place_id: place.id.clone(),
        place_name: place.name.to_owned(),
        description: place.description.to_owned(),
        stage,
        since_tick: since_tick.max(0) as u64,
        progress,
        fire,
        beds: home.then(|| beds_of(state, &place.id)),
        residents: if home {
            living(state).filter(|c| c.home_id == place.id).map(person).collect()
        } else {
            Vec::new()
        },
        present,
        project,
        stockpile,
        history: state
            .chronicle
            .iter()
            .rev()
            .filter(|entry| entry.ids.iter().any(|entry_id| entry_id == id))
            .take(SITE_HISTORY)
            .map(chronicle_entry)
            .collect(),
    })
}

/// A percentage in steps of five, as the wire rounds fires.
fn round5(value: f64) -> u8 {
    (js_round(value.clamp(0.0, 100.0) / 5.0) * 5.0) as u8
}

pub fn chronicle_tail(state: &SimState) -> Vec<ChronicleEntry> {
    let start = state.chronicle.len().saturating_sub(CHRONICLE_TAIL);
    state.chronicle[start..].iter().map(chronicle_entry).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use fsw_sim::{Command, Sim, create_state};
    use fsw_testing::create_test_state;

    #[test]
    fn status_reports_the_founding_until_the_village_is_full() {
        let extras = || StatusExtras { llm: crate::world_task::llm_off(), viewers: 0, speed: 1, throttled: false };
        let mut sim = Sim::new(create_state(4));
        assert_eq!(status_of(&sim.state, extras()).founding, Some(Founding { settled: 0, total: 13 }));
        sim.enqueue(Command::FoundVillage);
        sim.step();
        assert_eq!(status_of(&sim.state, extras()).founding, None);
        assert_eq!(status_of(&create_test_state(4), extras()).founding, None);
    }

    #[test]
    fn a_site_page_names_the_place_and_who_lives_there() {
        let state = create_test_state(1);
        let home = site_of(&state, "b:home_4").unwrap();
        assert_eq!(
            (home.kind, home.stage.as_str(), home.since_tick, home.beds),
            (SiteKind::Building, "whole", 0, Some(2))
        );
        let residents: Vec<&str> = home.residents.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(residents, ["Elowen", "Theron"]);
        assert!(home.project.is_none() && home.stockpile.is_none() && home.history.is_empty());
        let tavern = site_of(&state, "b:tavern").unwrap();
        assert_eq!(
            (tavern.name.as_str(), tavern.place_name.as_str(), tavern.beds),
            ("The Gilded Tankard", "The Gilded Tankard", None)
        );
        assert!(tavern.residents.is_empty() && !tavern.description.is_empty());
        // Villagers, places without a building, trees and nothing at all have no site page.
        for id in ["c1", "b:green", "b:nowhere", "s:3", "n:404"] {
            assert!(site_of(&state, id).is_none(), "{id}");
        }
        assert!(detail_json(&state, "c1").unwrap().contains("\"backstory\""));
        assert!(detail_json(&state, "b:tavern").unwrap().contains("\"placeName\""));
    }

    #[test]
    fn a_site_page_shows_its_condition_and_the_work_on_it() {
        use fsw_sim::state::{
            BuildTarget, BuildingStage, BuiltSpec, ChronicleEntry as SimChronicleEntry,
            ChronicleKind as SimChronicleKind, House, Project, Store,
        };
        use fsw_sim::systems::world::put_object;
        use fsw_world::places::get_place;

        let mut state = create_test_state(1);
        let raising = SimWorldObject::Building {
            stage: BuildingStage::Raising { target: BuildTarget::Same },
            since_tick: 5,
            work: 300.0,
            fire: 0.0,
            helpers: Vec::new(),
        };
        put_object(&mut state, "b:smithy", raising);
        let mut cost = Store::default();
        cost.add(Resource::Wood, 10, i64::MAX);
        cost.add(Resource::Stone, 4, i64::MAX);
        let mut delivered = Store::default();
        delivered.add(Resource::Wood, 6, i64::MAX);
        state.projects.push(Project {
            id: "j:1".into(),
            kind: ProjectKind::Raise,
            target: "b:smithy".into(),
            cost,
            delivered,
            need: 100.0,
            work: 25.0,
            since_tick: 5,
            helpers: vec!["c1".into()],
            sponsor: None,
            price: 0,
            pad_kind: None,
        });
        state.chronicle.push(SimChronicleEntry {
            tick: 9,
            kind: SimChronicleKind::World,
            text: "The village takes on raising the smithy.".into(),
            ids: vec!["b:smithy".into()],
        });
        state.stockpiles.entry("lumber_camp".into()).or_default().add(Resource::Wood, 7, i64::MAX);
        state.houses.insert(
            "home_4".into(),
            House { phase: HousePhase::Burning, fire: 42.0, built: 0.0, since_tick: 8, helpers: Vec::new() },
        );

        let smithy = site_of(&state, "b:smithy").unwrap();
        assert_eq!((smithy.stage.as_str(), smithy.since_tick, smithy.progress), ("raising", 5, 20));
        let project = smithy.project.unwrap();
        assert_eq!((project.label.as_str(), project.progress), ("raising the smithy", 25));
        assert_eq!(project.helpers, vec![Person { id: "c1".into(), name: "Aldric".into() }]);
        let needs: Vec<(&str, u32, u32)> =
            project.needs.iter().map(|n| (n.resource.as_str(), n.delivered, n.needed)).collect();
        assert_eq!(needs, [("wood", 6, 10), ("stone", 0, 4)]);
        assert_eq!(smithy.history.len(), 1);
        assert_eq!(site_of(&state, "b:lumber_camp").unwrap().stockpile, Some(Goods { wood: 7, stone: 0, ore: 0 }));
        let home = site_of(&state, "b:home_4").unwrap();
        assert_eq!((home.stage.as_str(), home.fire, home.since_tick), ("burning", 40, 8));

        let green = get_place("green");
        let spec = BuiltSpec {
            bundle: "halloween".into(),
            model: "bench".into(),
            x: green.origin.x,
            z: green.origin.z,
            rotation: 0.0,
            scale: 1.0,
            footprint: None,
            pad: None,
        };
        let bench = |stage| SimWorldObject::Built { stage, since_tick: 40, work: 0.0, fire: 0.0, spec: spec.clone() };
        put_object(&mut state, "n:9", bench(PropStage::Whole));
        let page = site_of(&state, "n:9").unwrap();
        assert_eq!(
            (page.kind, page.name.as_str(), page.place_id.as_str(), page.since_tick),
            (SiteKind::Built, "the bench near the village green", "green", 40)
        );
        assert!(page.beds.is_none() && page.present.is_empty());
        put_object(&mut state, "n:9", bench(PropStage::Gone));
        assert!(site_of(&state, "n:9").is_none());
    }

    #[test]
    fn a_child_is_whole_at_half_the_class_hit_points() {
        let state = create_test_state(1);
        let pip = living(&state).find(|c| c.profile.name == "Pip Vane").unwrap();
        let row = roster_of(&state).into_iter().find(|row| row.id == pip.id).unwrap();
        assert_eq!(row.hp, 1.0);
        assert_eq!(row.role, "child");
        assert!(row.child);
        assert_eq!(crate::encode::character_static(pip).max_hp, pip.class_id.def().max_hp * 0.5);
        assert_eq!(mood_of(pip), Mood::Ok);
        let detail = detail_of(&state, &pip.id).unwrap();
        assert!(detail.child);
        let aldric = living(&state).find(|c| c.profile.name == "Aldric Vane").unwrap();
        assert!(!roster_of(&state).into_iter().find(|row| row.id == aldric.id).unwrap().child);
        assert!(!detail_of(&state, &aldric.id).unwrap().child);
    }

    #[test]
    fn renders_the_inspector_and_roster() {
        let mut sim = Sim::new(create_test_state(4));
        for _ in 0..2000 {
            sim.step();
        }
        let roster = roster_of(&sim.state);
        assert_eq!(roster.len(), 18);
        assert!(roster.iter().all(|row| !row.status.is_empty()));
        let id = sim.state.order[0].clone();
        let detail = detail_of(&sim.state, &id).unwrap();
        assert_eq!(detail.plan.iter().filter(|row| row.current).count(), 1);
        assert!(!detail.relations.is_empty());
        assert!(detail.memories.len() <= DETAIL_MEMORIES);
        assert!(detail_of(&sim.state, "nobody").is_none());
    }
}

/// The digest task: four times a second, the status line and roster (sent at
/// most once a second, when changed) and the inspector for every watched
/// character, computed once however many viewers watch it.
pub mod task {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;
    use std::time::{Duration, Instant};

    use tokio_util::sync::CancellationToken;

    use super::{StatusExtras, detail_json, roster_of, status_of};
    use crate::AppState;
    use crate::world_task::Outgoing;

    pub const PERIOD: Duration = Duration::from_millis(250);
    pub const STATUS_MIN: Duration = Duration::from_secs(1);
    /// A watch nobody has refreshed (no open stream, no POST) for this long is dropped.
    pub const WATCH_TTL: Duration = Duration::from_secs(600);

    /// Runs one digest pass now; the task calls this every [`PERIOD`].
    pub fn digest(app: &AppState, last_status_at: &mut Option<Instant>) {
        let world = &app.world;
        let now = Instant::now();
        let watched: HashSet<String> = {
            let connected = world.connected.lock();
            let mut watches = world.watches.lock();
            watches.retain(|viewer, watch| connected.contains_key(viewer) || now.duration_since(watch.at) < WATCH_TTL);
            watches.values().filter_map(|watch| watch.id.clone()).collect()
        };
        let due_status = last_status_at.is_none_or(|at| now.duration_since(at) >= STATUS_MIN);
        let (status, roster, details) = {
            let core = world.core.read();
            let state = &core.sim.state;
            let status = due_status.then(|| {
                let extras = StatusExtras {
                    llm: world.llm.lock().clone(),
                    viewers: app.viewers.load(Ordering::Relaxed),
                    speed: core.speed,
                    throttled: world.throttled.load(Ordering::Relaxed),
                };
                (serde_json::to_string(&status_of(state, extras)), serde_json::to_string(&roster_of(state)))
            });
            let details: Vec<(String, Option<String>)> =
                watched.into_iter().map(|id| (id.clone(), detail_json(state, &id))).collect();
            (status.as_ref().and_then(|s| s.0.as_ref().ok().cloned()), status.and_then(|s| s.1.ok()), details)
        };
        let mut changed = false;
        if let Some(status) = status
            && world.status.lock().as_deref() != Some(status.as_str())
        {
            let status: Arc<str> = Arc::from(status);
            *world.status.lock() = Some(status.clone());
            let _ = world.outgoing.send(Outgoing::Status(status));
            changed = true;
        }
        if let Some(roster) = roster
            && world.roster.lock().as_deref() != Some(roster.as_str())
        {
            let roster: Arc<str> = Arc::from(roster);
            *world.roster.lock() = Some(roster.clone());
            let _ = world.outgoing.send(Outgoing::Roster(roster));
            changed = true;
        }
        if changed {
            *last_status_at = Some(now);
        }
        for (id, json) in details {
            let mut cache = world.details.lock();
            match json {
                Some(json) if cache.get(&id).map(|j| &**j) != Some(json.as_str()) => {
                    let json: Arc<str> = Arc::from(json);
                    cache.insert(id.clone(), json.clone());
                    drop(cache);
                    let _ = world.outgoing.send(Outgoing::Detail { id, json });
                }
                Some(_) => {}
                None => {
                    cache.remove(&id);
                }
            }
        }
    }

    pub fn spawn(app: Arc<AppState>, shutdown: CancellationToken) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(PERIOD);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let mut last_status_at = None;
            loop {
                tokio::select! {
                    () = shutdown.cancelled() => break,
                    _ = interval.tick() => digest(&app, &mut last_status_at),
                }
            }
        })
    }
}
