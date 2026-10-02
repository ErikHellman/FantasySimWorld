//! Phase 0: the committed browser exports are well-formed. Phase 2 deserialises
//! them into the ported `SimState` (and keeps doing so for good).

use serde_json::Value;

fn fixtures() -> Vec<(String, Value)> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .map(|path| {
            let text = std::fs::read_to_string(&path).unwrap();
            (path.file_name().unwrap().to_string_lossy().into_owned(), serde_json::from_str(&text).unwrap())
        })
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}

#[test]
fn exports_are_version_1_saves() {
    let found = fixtures();
    assert!(found.len() >= 2, "expected the committed export fixtures");
    for (name, save) in found {
        assert_eq!(save["version"], 1, "{name}");
        assert!(save["savedAt"].is_u64(), "{name}");
        let state = &save["state"];
        for key in [
            "seed",
            "rng",
            "tick",
            "nextId",
            "characters",
            "order",
            "monsters",
            "conversations",
            "graves",
            "corpses",
            "dead",
            "chronicle",
            "slots",
            "director",
            "tuning",
        ] {
            assert!(!state[key].is_null(), "{name}: state.{key}");
        }
        // The RNG is a u32 and must survive as one.
        assert!(state["rng"].as_u64().is_some_and(|rng| rng <= u64::from(u32::MAX)), "{name}");
        let order = state["order"].as_array().unwrap();
        assert!(order.iter().all(|id| state["characters"][id.as_str().unwrap()].is_object()), "{name}");
    }
}
