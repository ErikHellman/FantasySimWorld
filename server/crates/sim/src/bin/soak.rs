//! Headless run of the simulation with the local routine brain only (the Rust
//! side of `tools/soak.ts`, with the same flags and output).
//!
//!   cargo run --release -p fsw-sim --bin soak -- [days] [seed] [--log | --json | --trace]
//!
//! `--trace` prints one line per tick (tick, RNG state, every position to 1e-6
//! and every intent kind); diff it against `bun tools/soak.ts … --trace` (at the
//! `ts-sim-reference` tag) to find the first tick where the two implementations part ways.

use std::collections::BTreeMap;
use std::io::Write;
use std::time::Instant;

use fsw_core::time::{TICKS_PER_DAY, format_clock, game_time};
use fsw_sim::util::living;
use fsw_sim::{Command, Sim, SimState, create_state};
use serde_json::{Value, json};

fn trace_line(state: &SimState) -> String {
    let mut line = format!("{} {}", state.tick, state.rng);
    for id in &state.order {
        let c = &state.characters[id];
        let kind = c.intent.as_ref().map_or("-", |i| i.action.kind());
        line.push_str(&format!(" {id}:{:.6},{:.6},{kind}", c.pos.x, c.pos.z));
    }
    for (id, m) in &state.monsters {
        line.push_str(&format!(" {id}:{:.6},{:.6},{}", m.pos.x, m.pos.z, m.phase.as_str()));
    }
    line
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let positional: Vec<&String> = args.iter().filter(|a| !a.starts_with("--")).collect();
    let days: i64 = positional.first().map_or(10, |d| d.parse().expect("days"));
    let seed: u32 = positional.get(1).map_or(1337, |s| s.parse().expect("seed"));
    let flag = |name: &str| args.iter().any(|a| a == name);
    let (log, json_out, trace) = (flag("--log"), flag("--json"), flag("--trace"));

    // The village the sim invents for itself, as a server without a model would have.
    let mut sim = Sim::new(create_state(seed));
    sim.enqueue(Command::FoundVillage);
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    let started = Instant::now();
    let mut chronicle_seen = sim.state.chronicle.len();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());

    for _ in 0..days * TICKS_PER_DAY {
        for event in sim.step() {
            let key = match &event {
                fsw_sim::SimEvent::ConversationStarted { tone, .. } => format!("conversation:{}", tone.as_str()),
                fsw_sim::SimEvent::Hit { outcome: fsw_sim::events::HitOutcome::Blocked, .. } => {
                    "hit:blocked".to_owned()
                }
                fsw_sim::SimEvent::Hit { outcome: fsw_sim::events::HitOutcome::Dodged, .. } => "hit:dodged".to_owned(),
                fsw_sim::SimEvent::Death { monster, .. } => {
                    (if *monster { "death:monster" } else { "death:villager" }).to_owned()
                }
                other => other.kind().to_owned(),
            };
            *counts.entry(key).or_default() += 1;
        }
        if trace {
            writeln!(out, "{}", trace_line(&sim.state)).unwrap();
        }
        if log {
            let chronicle = &sim.state.chronicle;
            while chronicle_seen < chronicle.len() {
                let entry = &chronicle[chronicle_seen];
                let time = game_time(entry.tick);
                writeln!(
                    out,
                    "D{} {} [{}] {}",
                    time.day,
                    format_clock(time.minute_of_day),
                    entry.kind.as_str(),
                    entry.text
                )
                .unwrap();
                chronicle_seen += 1;
            }
            if chronicle.len() >= 590 {
                chronicle_seen = chronicle_seen.min(chronicle.len());
            }
        }
    }

    let elapsed = started.elapsed().as_secs_f64();
    let state = &sim.state;
    let mut kinds: BTreeMap<&str, u64> = BTreeMap::new();
    for entry in &state.chronicle {
        *kinds.entry(entry.kind.as_str()).or_default() += 1;
    }
    let labels_of = |id: &str| {
        let mut labels: BTreeMap<&str, u64> = BTreeMap::new();
        for (other, r) in &state.characters[id].relationships {
            if state.characters.contains_key(other) {
                *labels.entry(r.label.as_str()).or_default() += 1;
            }
        }
        labels
    };

    if trace {
        // The trace is the whole output.
    } else if json_out {
        let report = json!({
            "days": days,
            "seed": seed,
            "tick": state.tick,
            "rng": state.rng,
            "elapsedSeconds": (elapsed * 100.0).round() / 100.0,
            "events": counts,
            "alive": state.order.len(),
            "dead": state.dead.len(),
            "arrivals": state.director.arrivals,
            "raids": state.director.raids,
            "deaths": state.dead.iter().map(|(id, r)| json!({"id": id, "name": r.name, "day": r.day, "cause": r.cause})).collect::<Vec<Value>>(),
            "chronicleKinds": kinds,
            "stockpiles": state.stockpiles,
            "economy": state.economy,
            "objects": state.objects.len(),
            "characters": living(state).map(|c| json!({
                "id": c.id,
                "name": c.profile.name,
                "hp": c.hp.round(),
                "hunger": c.needs.hunger.round(),
                "energy": c.needs.energy.round(),
                "social": c.needs.social.round(),
                "kills": c.kills,
                "memories": c.memories.len(),
                "inventory": c.inventory,
                "carried": c.carried,
                "gold": c.gold,
                "partner": c.partner_id,
                "labels": labels_of(&c.id),
            })).collect::<Vec<Value>>(),
        });
        writeln!(out, "{report}").unwrap();
    } else {
        let ticks = (days * TICKS_PER_DAY) as f64;
        writeln!(out, "\n{days} days in {elapsed:.1} s ({} ticks/s)", (ticks / elapsed).round()).unwrap();
        writeln!(out, "events {counts:?}").unwrap();
        writeln!(out, "alive {}, dead {}, arrivals {}", state.order.len(), state.dead.len(), state.director.arrivals)
            .unwrap();
        for record in state.dead.values() {
            writeln!(out, "  † {}, day {}: {}", record.name, record.day, record.cause).unwrap();
        }
        writeln!(out, "chronicle kinds (last 600) {kinds:?}").unwrap();
        writeln!(out, "chest {} gold, stalls hold {}", state.economy.purse, state.economy.describe_stock()).unwrap();
        for c in living(state) {
            let partner = c
                .partner_id
                .as_ref()
                .and_then(|p| state.characters.get(p))
                .map_or(String::new(), |p| format!(" ♥{}", p.profile.name));
            writeln!(
                out,
                "{:<20} hp {:>3} hunger {:>3} energy {:>3} social {:>3} kills {} mem {} food {} potion {} gift {} gold {}{partner} {:?}",
                c.profile.name,
                c.hp.round(),
                c.needs.hunger.round(),
                c.needs.energy.round(),
                c.needs.social.round(),
                c.kills,
                c.memories.len(),
                c.inventory.food,
                c.inventory.potion,
                c.inventory.gift,
                c.gold,
                labels_of(&c.id)
            )
            .unwrap();
        }
    }
}
