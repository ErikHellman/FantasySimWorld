//! The Rust world against the TypeScript world's output (`tests/golden/*.json`,
//! written by `bun tools/dump-world.ts`): categories, nav costs and paths
//! exactly, coordinates to 1e-9. The sample points and pairs are replayed from
//! the dump's own RNG, so the RNG port is checked along the way.

use base64::Engine;
use fsw_core::rng::{next_float, pick_index, range};
use fsw_core::{Vec2, vec2};
use fsw_world::hex::hex_at;
use fsw_world::map::{Ground, ground_at, is_on_road, road_distance, world_map};
use fsw_world::navgrid::{CELL, nav_grid, nearest_walkable, world_hash};
use fsw_world::pathfind::{find_path, find_route};
use fsw_world::places::places;
use fsw_world::scatter::scatter;
use serde::Serialize;
use serde_json::Value;

/// `FIXTURE_SEED` in tools/dump-world.ts.
const FIXTURE_SEED: u32 = 7_000_001;
const POINT_SAMPLES: usize = 2000;
const PATH_PAIRS: usize = 300;
const TOLERANCE: f64 = 1e-9;

fn golden(name: &str) -> Value {
    let path = format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e} (run `bun tools/dump-world.ts`)"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

/// Structural equality with numbers compared to within `TOLERANCE`.
fn assert_close(actual: &Value, expected: &Value, at: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (a.as_f64().unwrap(), e.as_f64().unwrap());
            assert!((a - e).abs() <= TOLERANCE, "{at}: {a} != {e}");
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(a.len(), e.len(), "{at}: length");
            for (i, (a, e)) in a.iter().zip(e).enumerate() {
                assert_close(a, e, &format!("{at}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(e)) => {
            let keys = |m: &serde_json::Map<String, Value>| m.keys().cloned().collect::<Vec<_>>();
            assert_eq!(keys(a), keys(e), "{at}: keys");
            for (key, e) in e {
                assert_close(&a[key], e, &format!("{at}.{key}"));
            }
        }
        _ => assert_eq!(actual, expected, "{at}"),
    }
}

fn to_value(value: impl Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn random_points(rng: &mut u32, count: usize) -> Vec<Vec2> {
    let map = world_map();
    (0..count)
        .map(|_| {
            let x = range(rng, map.min_x - 4.0, map.max_x + 4.0);
            let z = range(rng, map.min_z - 4.0, map.max_z + 4.0);
            vec2(x, z)
        })
        .collect()
}

fn point(value: &Value) -> Vec2 {
    vec2(value["x"].as_f64().unwrap(), value["z"].as_f64().unwrap())
}

#[test]
fn map_matches() {
    let golden = golden("map.json");
    let mut actual = to_value(world_map());
    actual["hexAt"] = golden["hexAt"].clone();
    assert_close(&actual, &golden, "map");

    let mut rng = FIXTURE_SEED;
    let points = random_points(&mut rng, POINT_SAMPLES);
    for (p, sample) in points.iter().zip(golden["hexAt"].as_array().unwrap()) {
        assert_eq!((p.x, p.z), (sample["x"].as_f64().unwrap(), sample["z"].as_f64().unwrap()), "fixture RNG replay");
        let hex = hex_at(p.x, p.z);
        assert_eq!(to_value(hex), sample["hex"], "hexAt({}, {})", p.x, p.z);
    }
}

#[test]
fn ground_and_roads_match() {
    let golden = golden("ground.json");
    let step = golden["step"].as_f64().unwrap();
    let (origin_x, origin_z) = (golden["originX"].as_f64().unwrap(), golden["originZ"].as_f64().unwrap());
    let (nx, nz) = (golden["nx"].as_u64().unwrap(), golden["nz"].as_u64().unwrap());
    let mut ground = String::new();
    let mut road = String::new();
    for iz in 0..nz {
        for ix in 0..nx {
            let x = origin_x + ix as f64 * step;
            let z = origin_z + iz as f64 * step;
            ground.push(match ground_at(x, z) {
                Ground::Void => 'v',
                Ground::Blocked => 'b',
                Ground::Water => 'w',
                Ground::Open => 'o',
            });
            road.push(if is_on_road(x, z) { '1' } else { '0' });
        }
    }
    let mismatches = |a: &str, b: &str| a.chars().zip(b.chars()).filter(|(a, b)| a != b).count();
    assert_eq!(mismatches(&ground, golden["ground"].as_str().unwrap()), 0, "ground mask");
    assert_eq!(mismatches(&road, golden["road"].as_str().unwrap()), 0, "road mask");

    let mut rng = FIXTURE_SEED;
    random_points(&mut rng, POINT_SAMPLES);
    let mut expected = golden["roadDistances"].as_array().unwrap().iter();
    for p in random_points(&mut rng, POINT_SAMPLES) {
        let d = road_distance(p.x, p.z);
        if d.is_finite() {
            let sample = expected.next().expect("a finite road distance the fixture left out");
            assert_eq!((p.x, p.z), (sample["x"].as_f64().unwrap(), sample["z"].as_f64().unwrap()));
            assert!((d - sample["d"].as_f64().unwrap()).abs() <= TOLERANCE, "roadDistance({}, {})", p.x, p.z);
        }
    }
    assert!(expected.next().is_none(), "the fixture has road distances the port finds infinite");
}

#[test]
fn nav_grid_matches_bit_for_bit() {
    let golden = golden("navgrid.json");
    let nav = nav_grid();
    assert_eq!(CELL, golden["cell"].as_f64().unwrap());
    assert_eq!(nav.origin_x, golden["originX"].as_f64().unwrap());
    assert_eq!(nav.origin_z, golden["originZ"].as_f64().unwrap());
    assert_eq!(
        (nav.width as u64, nav.height as u64),
        (golden["width"].as_u64().unwrap(), golden["height"].as_u64().unwrap())
    );
    let bytes = base64::engine::general_purpose::STANDARD.decode(golden["costF32Base64"].as_str().unwrap()).unwrap();
    let (words, rest) = bytes.as_chunks::<4>();
    assert!(rest.is_empty());
    let expected: Vec<u32> = words.iter().map(|&w| u32::from_le_bytes(w)).collect();
    let actual: Vec<u32> = nav.cost.iter().map(|c| c.to_bits()).collect();
    let wrong: Vec<usize> = (0..expected.len()).filter(|&i| expected[i] != actual[i]).collect();
    assert!(wrong.is_empty(), "{} nav cells differ, first at {:?}", wrong.len(), wrong.first());
    assert_eq!(nav.cost.iter().filter(|&&c| c > 0.0).count() as u64, golden["walkable"].as_u64().unwrap());
    assert_eq!(world_hash(), golden["hash"].as_str().unwrap());
}

#[test]
fn places_match() {
    assert_close(&to_value(places()), &golden("places.json"), "places");
}

#[test]
fn scatter_matches() {
    assert_close(&to_value(scatter()), &golden("scatter.json"), "scatter");
}

#[test]
fn paths_match() {
    let golden = golden("paths.json");
    let mut targets: Vec<(String, Vec2)> = Vec::new();
    for place in places() {
        targets.push((format!("{}:door", place.id), place.door));
        for slot in &place.slots {
            targets.push((slot.id.clone(), slot.pos));
        }
        for (i, &pos) in place.spawn_points.iter().enumerate() {
            targets.push((format!("{}:spawn{i}", place.id), pos));
        }
    }

    let mut rng = FIXTURE_SEED;
    random_points(&mut rng, POINT_SAMPLES);
    random_points(&mut rng, POINT_SAMPLES);
    let map = world_map();
    let pairs = golden["pairs"].as_array().unwrap();
    for (i, pair) in pairs.iter().enumerate() {
        let (from_label, from) = if i < PATH_PAIRS {
            let (label, pos) = &targets[pick_index(&mut rng, targets.len())];
            (label.clone(), *pos)
        } else {
            let x = range(&mut rng, map.min_x, map.max_x);
            let z = range(&mut rng, map.min_z, map.max_z);
            ("random".to_owned(), vec2(x, z))
        };
        let (to_label, to) = &targets[pick_index(&mut rng, targets.len())];
        assert_eq!(
            (from_label.as_str(), to_label.as_str()),
            (pair["from"].as_str().unwrap(), pair["to"].as_str().unwrap())
        );
        assert_eq!((from, *to), (point(&pair["fromPos"]), point(&pair["toPos"])), "pair {i}");
        assert_close(
            &to_value(find_route(from, *to)),
            &pair["route"],
            &format!("pair {i} ({from_label} -> {to_label}) route"),
        );
        assert_close(&to_value(find_path(from, *to)), &pair["path"], &format!("pair {i} path"));
    }

    let points = random_points(&mut rng, POINT_SAMPLES);
    for (p, sample) in points.into_iter().zip(golden["nearest"].as_array().unwrap()) {
        let max_radius = 1 + (next_float(&mut rng) * 8.0).floor() as i64;
        assert_eq!(max_radius, sample["maxRadius"].as_i64().unwrap());
        assert_close(&to_value(nearest_walkable(p, max_radius)), &sample["nearest"], "nearestWalkable");
    }
}
