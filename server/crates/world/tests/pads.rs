//! The pads the village may build on stand on open, walkable ground, clear of everything the map put there.

use fsw_world::navgrid::{AGENT_RADIUS, nearest_walkable};
use fsw_world::places::{obstacle_contains, pads, places};
use fsw_world::scatter::scatter;

#[test]
fn every_pad_is_on_clear_walkable_ground() {
    let all = pads();
    assert!(all.len() >= 20, "{} pads", all.len());
    for pad in &all {
        assert_eq!(nearest_walkable(pad.pos, 0), Some(pad.pos), "{} is not walkable at {:?}", pad.id, pad.pos);
        let blocked = places()
            .iter()
            .flat_map(|p| p.obstacles.iter())
            .chain(scatter().obstacles.iter())
            .any(|o| obstacle_contains(o, pad.pos.x, pad.pos.z, AGENT_RADIUS + 0.6));
        assert!(!blocked, "{} at {:?} stands in something", pad.id, pad.pos);
        assert!(!pad.kinds.is_empty());
    }
    let mut ids: Vec<&str> = all.iter().map(|p| p.id.as_str()).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "pad ids are unique");
}
