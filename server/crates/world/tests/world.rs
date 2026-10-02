//! Port of `src/world/world.test.ts` (the slot-animation check moves to the sim, which owns the animation table).

use std::collections::HashSet;

use fsw_core::jsmath::js_hypot;
use fsw_world::navgrid::{is_walkable, nearest_walkable};
use fsw_world::pathfind::{find_path, path_length};
use fsw_world::places::{get_place, homes, places};
use fsw_world::scatter::scatter;

#[test]
fn places_have_unique_ids_and_reachable_doors() {
    let ids: HashSet<&str> = places().iter().map(|p| p.id.as_str()).collect();
    assert_eq!(ids.len(), places().len());
    for place in places() {
        assert!(nearest_walkable(place.door, 3).is_some(), "{} door", place.id);
    }
}

#[test]
fn places_have_beds_for_the_whole_cast_and_a_few_newcomers() {
    assert!(homes().iter().map(|home| home.beds).sum::<u32>() >= 14);
}

#[test]
fn every_activity_slot_is_next_to_walkable_ground() {
    for place in places() {
        for slot in &place.slots {
            assert!(nearest_walkable(slot.pos, 3).is_some(), "{}", slot.id);
        }
        for &point in &place.spawn_points {
            assert!(nearest_walkable(point, 3).is_some(), "{} spawn", place.id);
        }
    }
}

#[test]
fn navigation_connects_every_pair_of_places() {
    let tavern = get_place("tavern");
    for place in places() {
        assert!(find_path(tavern.door, place.door).is_some(), "tavern -> {}", place.id);
        assert!(find_path(place.door, tavern.door).is_some(), "{} -> tavern", place.id);
    }
}

#[test]
fn navigation_crosses_the_river_over_the_bridge() {
    let smithy = get_place("smithy");
    let graveyard = get_place("graveyard");
    let path = find_path(smithy.door, graveyard.door).expect("a path");
    let direct = js_hypot(graveyard.door.x - smithy.door.x, graveyard.door.z - smithy.door.z);
    assert!(path_length(smithy.door, &path) < direct * 1.6);
    assert!(path.iter().all(|p| is_walkable(p.x, p.z)));
}

#[test]
fn scatter_is_deterministic_decoration() {
    assert!(scatter().models.len() > 1000);
}
