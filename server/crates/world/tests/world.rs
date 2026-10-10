//! Port of `src/world/world.test.ts` (the slot-animation check moves to the sim, which owns the animation table).

use std::collections::HashSet;

use fsw_core::jsmath::js_hypot;
use fsw_world::navgrid::{AGENT_RADIUS, is_walkable, nearest_walkable};
use fsw_world::pathfind::{find_path, path_length};
use fsw_world::places::{
    ActivityId, billboards, find_billboard, get_place, get_slot, homes, obstacle_contains, places,
};
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

#[test]
fn billboards_have_unique_ids_and_two_reading_spots_each() {
    let ids: HashSet<&str> = billboards().iter().map(|b| b.id.as_str()).collect();
    assert_eq!(ids.len(), billboards().len());
    assert_eq!(billboards().len(), 7);
    for board in billboards() {
        assert!(find_billboard(&board.id).is_some());
        assert_eq!(board.slot_ids.len(), 2, "{}", board.id);
        for id in &board.slot_ids {
            let slot = get_slot(id);
            assert_eq!(slot.place_id, board.place_id, "{id}");
            assert_eq!(slot.activities, vec![ActivityId::ReadBoard], "{id}");
        }
    }
    let reading = places().iter().flat_map(|p| &p.slots).filter(|s| s.activities.contains(&ActivityId::ReadBoard));
    assert_eq!(reading.count(), billboards().len() * 2);
}

#[test]
fn billboard_reading_spots_are_walkable_and_reachable() {
    let tavern = get_place("tavern");
    for board in billboards() {
        let place = get_place(&board.place_id);
        for id in &board.slot_ids {
            let pos = get_slot(id).pos;
            assert!(is_walkable(pos.x, pos.z), "{id} is not walkable");
            assert!(find_path(place.door, pos).is_some(), "{} door -> {id}", place.id);
            assert!(find_path(tavern.door, pos).is_some(), "tavern -> {id}");
        }
    }
}

#[test]
fn billboards_stand_clear_of_slots_doors_and_spawn_points() {
    for board in billboards() {
        let place = get_place(&board.place_id);
        let footprint = place
            .obstacles
            .iter()
            .find(|o| o.x() == board.pos.x && o.z() == board.pos.z)
            .unwrap_or_else(|| panic!("{} has no footprint", board.id));
        for other in places() {
            for slot in &other.slots {
                assert!(
                    !obstacle_contains(footprint, slot.pos.x, slot.pos.z, AGENT_RADIUS),
                    "{} covers {}",
                    board.id,
                    slot.id
                );
            }
            assert!(
                !obstacle_contains(footprint, other.door.x, other.door.z, AGENT_RADIUS),
                "{} covers {} door",
                board.id,
                other.id
            );
            for p in &other.spawn_points {
                assert!(!obstacle_contains(footprint, p.x, p.z, AGENT_RADIUS), "{} covers a spawn point", board.id);
            }
        }
    }
}
