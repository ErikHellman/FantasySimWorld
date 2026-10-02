//! The committed browser exports load into the Rust `SimState`, survive a
//! round trip unchanged, and keep running (for good: these are permanent).

use fsw_sim::{SaveFile, Sim};
use indexmap::IndexMap;
use serde::de::IgnoredAny;
use serde_json::Value;

fn fixtures() -> Vec<(String, String)> {
    let dir = format!("{}/tests/fixtures", env!("CARGO_MANIFEST_DIR"));
    let mut found: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "json"))
        .map(|path| (path.file_name().unwrap().to_string_lossy().into_owned(), std::fs::read_to_string(&path).unwrap()))
        .collect();
    found.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(found.len() >= 2, "expected the committed export fixtures");
    found
}

/// JSON equality where `70` and `70.0` are the same number.
fn same(a: &Value, b: &Value, at: &str) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => assert_eq!(x.as_f64(), y.as_f64(), "{at}"),
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{at}: length");
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                same(x, y, &format!("{at}[{i}]"));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            let keys = |m: &serde_json::Map<String, Value>| {
                let mut keys: Vec<_> = m.keys().cloned().collect();
                keys.sort();
                keys
            };
            assert_eq!(keys(x), keys(y), "{at}: keys");
            for (key, value) in x {
                same(value, &y[key], &format!("{at}.{key}"));
            }
        }
        _ => assert_eq!(a, b, "{at}"),
    }
}

#[test]
fn exports_load_and_round_trip_unchanged() {
    for (name, text) in fixtures() {
        let save: SaveFile = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(save.version, 1, "{name}");
        let original: Value = serde_json::from_str(&text).unwrap();
        same(&serde_json::to_value(&save).unwrap(), &original, &name);
        // Key order of the id maps is part of the state (it decides iteration order).
        #[derive(serde::Deserialize)]
        struct Keys {
            state: KeyedState,
        }
        #[derive(serde::Deserialize)]
        struct KeyedState {
            characters: IndexMap<String, IgnoredAny>,
            monsters: IndexMap<String, IgnoredAny>,
        }
        let keys: Keys = serde_json::from_str(&text).unwrap();
        assert!(save.state.characters.keys().eq(keys.state.characters.keys()), "{name}");
        assert!(save.state.monsters.keys().eq(keys.state.monsters.keys()), "{name}");
    }
}

#[test]
fn exports_keep_running() {
    for (name, text) in fixtures() {
        let save: SaveFile = serde_json::from_str(&text).unwrap();
        let mut sim = Sim::new(save.state);
        for _ in 0..4000 {
            sim.step();
        }
        assert!(sim.state.order.len() >= 7, "{name}");
    }
}
