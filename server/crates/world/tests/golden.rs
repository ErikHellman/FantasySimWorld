//! Phase 0: the golden fixtures are readable and describe the world the plan
//! measured. Phase 1 replaces these checks with comparisons against the port.

use base64::Engine;
use serde_json::Value;

fn golden(name: &str) -> Value {
    let path = format!("{}/tests/golden/{name}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e} (run `bun tools/dump-world.ts`)"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn len(value: &Value) -> usize {
    value.as_array().expect("an array").len()
}

#[test]
fn map_has_every_hex() {
    let map = golden("map.json");
    let (cols, rows) = (map["cols"].as_u64().unwrap(), map["rows"].as_u64().unwrap());
    assert_eq!((cols, rows), (19, 15));
    assert_eq!(len(&map["cells"]), 285);
    assert_eq!(len(&map["hexAt"]), 2000);
}

#[test]
fn places_slots_and_scatter() {
    let places = golden("places.json");
    assert_eq!(len(&places), 23);
    let slots: usize = places.as_array().unwrap().iter().map(|p| len(&p["slots"])).sum();
    assert_eq!(slots, 67);
    let scatter = golden("scatter.json");
    assert_eq!(len(&scatter["models"]), 3070);
}

#[test]
fn nav_grid_costs_decode() {
    let nav = golden("navgrid.json");
    let (width, height) = (nav["width"].as_u64().unwrap() as usize, nav["height"].as_u64().unwrap() as usize);
    assert_eq!((width, height), (273, 187));
    let bytes = base64::engine::general_purpose::STANDARD.decode(nav["costF32Base64"].as_str().unwrap()).unwrap();
    assert_eq!(bytes.len(), width * height * 4);
    let (words, rest) = bytes.as_chunks::<4>();
    assert!(rest.is_empty());
    let costs: Vec<f32> = words.iter().map(|&word| f32::from_le_bytes(word)).collect();
    let walkable = costs.iter().filter(|&&c| c > 0.0).count();
    assert_eq!(walkable as u64, nav["walkable"].as_u64().unwrap());
    // Road and grass are the only costs.
    assert!(costs.iter().all(|&c| c == 0.0 || c == 1.0 || c == 1.45f32));
}

#[test]
fn ground_and_paths() {
    let ground = golden("ground.json");
    let cells = (ground["nx"].as_u64().unwrap() * ground["nz"].as_u64().unwrap()) as usize;
    assert_eq!(ground["ground"].as_str().unwrap().len(), cells);
    assert_eq!(ground["road"].as_str().unwrap().len(), cells);
    let paths = golden("paths.json");
    assert_eq!(len(&paths["pairs"]), 340);
    assert_eq!(len(&paths["nearest"]), 2000);
}
