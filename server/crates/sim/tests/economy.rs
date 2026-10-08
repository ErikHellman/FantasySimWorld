//! Coin: villagers start with some, are paid for what they put down and sell, spend it on the
//! stalls, and a passing trader fills the village chest once a day. All of it is deterministic and
//! survives a save.

use fsw_core::time::TICKS_PER_DAY;
use fsw_sim::Command;
use fsw_sim::Sim;
use fsw_sim::commands::TuningPatch;
use fsw_sim::data::economy::{
    MAX_WAGE_PER_DEPOSIT, STARTING_PURSE, TRADER_HOUR, base_price, material_price, material_reserve, price,
    starting_gold,
};
use fsw_sim::state::{
    ActivityId, ChronicleKind, Economy, Intent, IntentAction, IntentSource, ItemId, Resource, SimState, WorldJob,
};
use fsw_sim::systems::director::tick_at;
use fsw_sim::systems::economy::wages_for;
use fsw_sim::util::living;
use fsw_testing::create_test_state;

fn quiet(seed: u32) -> Sim {
    let mut sim = Sim::new(create_test_state(seed));
    sim.state.director.next_wave_tick = i64::MAX;
    sim.enqueue(Command::Tuning { tuning: TuningPatch { chatter: Some(0.0), ..Default::default() } });
    sim
}

fn round_trip(state: &SimState) -> SimState {
    serde_json::from_str(&serde_json::to_string(state).unwrap()).unwrap()
}

fn run_until(sim: &mut Sim, ticks: i64, until: impl Fn(&SimState) -> bool) -> i64 {
    for i in 0..ticks {
        if until(&sim.state) {
            return i;
        }
        sim.step();
    }
    ticks
}

fn stock(state: &mut SimState, place: &str, resource: Resource, count: i64) {
    state.stockpiles.entry(place.into()).or_default().add(resource, count, i64::MAX);
}

fn set_intent(sim: &mut Sim, id: &str, action: IntentAction) {
    let epoch = sim.state.characters[id].epoch;
    sim.enqueue(Command::SetIntents { id: id.into(), epoch, intents: vec![Intent::new(action, IntentSource::Llm)] });
}

fn trade_lines(state: &SimState) -> usize {
    state.chronicle.iter().filter(|e| e.kind == ChronicleKind::Trade).count()
}

#[test]
fn founders_start_with_the_coin_of_their_trade_and_children_with_none() {
    let state = create_test_state(1);
    for c in living(&state) {
        let expected = if c.is_child() { 0 } else { starting_gold(c.class_id) };
        assert_eq!(c.gold, expected, "{}", c.profile.name);
        assert!(c.inventory.trinket == 0 && c.inventory.relic == 0);
    }
    assert!(state.economy.is_default());
    assert_eq!(state.economy.purse, STARTING_PURSE);
    let json = serde_json::to_string(&state).unwrap();
    assert!(!json.contains("\"economy\""), "an untouched chest is left out of a save");
    assert!(!json.contains("\"trinket\""), "empty valuables are left out of a save");
    assert_eq!(round_trip(&state), state);
}

#[test]
fn prices_rise_when_the_stalls_are_bare_and_fall_as_they_fill() {
    for item in ItemId::ALL {
        assert_eq!(price(item, 0), base_price(item) + 1);
        assert_eq!(price(item, 1), base_price(item));
        assert_eq!(price(item, 6), base_price(item) - 1);
        assert!(price(item, 1000) >= 1);
    }
    let mut load = fsw_sim::state::Store::default();
    load.add(Resource::Ore, 6, i64::MAX);
    assert_eq!(wages_for(&load), MAX_WAGE_PER_DEPOSIT, "a load of ore is worth more than the cap");
    let mut load = fsw_sim::state::Store::default();
    load.add(Resource::Wood, 4, i64::MAX);
    assert_eq!(wages_for(&load), 4 * material_price(Resource::Wood));
}

#[test]
fn a_stall_keeper_sells_the_surplus_of_the_pack_and_the_chest_pays_for_it() {
    let mut sim = quiet(2);
    let id = "c1".to_owned();
    {
        let c = sim.state.characters.get_mut(&id).unwrap();
        c.inventory.gift = 3;
        c.inventory.food = 2;
        c.inventory.trinket = 1;
    }
    let gold_before = sim.state.characters[&id].gold;
    set_intent(
        &mut sim,
        &id,
        IntentAction::Activity { activity: ActivityId::Trade, place_id: None, minutes: Some(30.0) },
    );
    let sold = |state: &SimState| state.characters["c1"].inventory.gift <= 1;
    assert!(run_until(&mut sim, 3 * TICKS_PER_DAY / 4, sold) < 3 * TICKS_PER_DAY / 4, "the stall never opened");
    let c = &sim.state.characters[&id];
    assert_eq!((c.inventory.gift, c.inventory.food, c.inventory.trinket), (1, 1, 0), "a gift and a meal are kept back");
    let earned = c.gold - gold_before;
    assert!(earned > 0);
    assert_eq!(sim.state.economy.purse, STARTING_PURSE - earned, "the chest paid for it");
    assert_eq!(sim.state.economy.in_stock(ItemId::Gift), 2);
    assert_eq!(sim.state.economy.in_stock(ItemId::Trinket), 1);
    assert!(c.outcome.starts_with("You sold "), "{}", c.outcome);
}

#[test]
fn shopping_costs_coin_and_bare_stalls_sell_nothing() {
    let mut sim = quiet(3);
    let id = "c1".to_owned();
    sim.state.characters.get_mut(&id).unwrap().inventory.food = 0;
    set_intent(&mut sim, &id, IntentAction::Activity { activity: ActivityId::Shop, place_id: None, minutes: None });
    let shopped = |state: &SimState| state.characters["c1"].outcome.contains("stalls");
    assert!(run_until(&mut sim, TICKS_PER_DAY / 2, shopped) < TICKS_PER_DAY / 2);
    let c = &sim.state.characters[&id];
    assert_eq!(c.outcome, "The market stalls were bare.");
    assert_eq!(c.inventory.food, 0, "nothing comes free any more");

    // With food on the stalls, a hungry shopper buys some at the day's price.
    sim.state.economy.stock_up(ItemId::Food, 2);
    let gold = sim.state.characters[&id].gold;
    let asked = price(ItemId::Food, 2);
    set_intent(&mut sim, &id, IntentAction::Activity { activity: ActivityId::Shop, place_id: None, minutes: None });
    let bought = |state: &SimState| state.characters["c1"].inventory.food > 0;
    assert!(run_until(&mut sim, TICKS_PER_DAY / 2, bought) < TICKS_PER_DAY / 2);
    let c = &sim.state.characters[&id];
    assert_eq!(c.gold, gold - asked);
    assert_eq!(sim.state.economy.in_stock(ItemId::Food), 1);
    assert_eq!(c.outcome, format!("You bought some food at the market for {asked} gold."));

    // Without the coin, the stalls are as good as bare.
    let shopper = sim.state.characters.get_mut(&id).unwrap();
    shopper.gold = 0;
    shopper.inventory.food = 0;
    sim.state.economy.stock_up(ItemId::Food, 5);
    set_intent(&mut sim, &id, IntentAction::Activity { activity: ActivityId::Shop, place_id: None, minutes: None });
    let refused = |state: &SimState| state.characters["c1"].outcome.contains("afford");
    assert!(run_until(&mut sim, TICKS_PER_DAY / 2, refused) < TICKS_PER_DAY / 2);
    assert_eq!(sim.state.characters[&id].inventory.food, 0);
}

#[test]
fn the_trader_comes_once_a_day_buys_the_surplus_and_pays_the_chest() {
    let mut sim = quiet(4);
    // Ore, which nothing the village builds yet spends, so the builders leave the piles alone.
    let reserve = material_reserve("market");
    stock(&mut sim.state, "market", Resource::Ore, reserve + 15);
    stock(&mut sim.state, "mine", Resource::Ore, material_reserve("mine") + 4);
    sim.state.economy.stock_up(ItemId::Gift, 4);
    sim.state.economy.stock_up(ItemId::Food, 1);
    let visit = tick_at(sim.state.tick / TICKS_PER_DAY + 1, TRADER_HOUR);
    run_until(&mut sim, TICKS_PER_DAY, |state| state.tick == visit - 1);
    assert_eq!(trade_lines(&sim.state), 0, "nothing before the visit");
    // Wages and sales move the chest all morning; what the trader adds is measured from here.
    let purse = sim.state.economy.purse;
    sim.step();
    assert_eq!(trade_lines(&sim.state), 1, "one line for the visit");
    let line = sim.state.chronicle.iter().find(|e| e.kind == ChronicleKind::Trade).unwrap();
    assert!(line.text.starts_with("A passing trader bought "), "{}", line.text);
    assert_eq!(sim.state.economy.in_stock(ItemId::Gift), 1, "the stalls keep a gift back");
    assert_eq!(sim.state.economy.in_stock(ItemId::Food), 1, "too little food to sell any");
    let market = &sim.state.stockpiles["market"];
    assert_eq!(market.get(Resource::Ore), reserve, "the market pile keeps its reserve");
    assert_eq!(sim.state.stockpiles["mine"].get(Resource::Ore), material_reserve("mine"), "and so does the mine");
    let expected = 3 * base_price(ItemId::Gift) + 19 * material_price(Resource::Ore);
    // The miners may have added to the piles that morning, so at least this much.
    assert!(sim.state.economy.purse >= purse + expected, "{} < {}", sim.state.economy.purse, purse + expected);
    // One visit a day at most, whatever the stall keepers sell meanwhile; the camp keeps the builders' logs.
    stock(&mut sim.state, "lumber_camp", Resource::Wood, 50);
    run_until(&mut sim, TICKS_PER_DAY, |_| false);
    assert!(trade_lines(&sim.state) <= 2);
    let camp = sim.state.stockpiles["lumber_camp"].get(Resource::Wood);
    assert!(camp <= material_reserve("lumber_camp"), "the surplus was sold: {camp}");
    assert!(camp > material_reserve("market"), "the builders' reserve is kept: {camp}");
}

#[test]
fn carriers_are_paid_for_what_they_put_down_while_the_chest_holds_anything() {
    let mut sim = quiet(5);
    let id = "c1".to_owned();
    let gold = sim.state.characters[&id].gold;
    sim.state.characters.get_mut(&id).unwrap().carried.add(Resource::Wood, 4, i64::MAX);
    set_intent(
        &mut sim,
        &id,
        IntentAction::WorldWork { job: WorldJob::Deliver, target: "lumber_camp".into(), minutes: None },
    );
    let delivered = |state: &SimState| state.characters["c1"].carried.is_empty();
    assert!(run_until(&mut sim, TICKS_PER_DAY / 2, delivered) < TICKS_PER_DAY / 2);
    let c = &sim.state.characters[&id];
    assert_eq!(c.gold, gold + 4 * material_price(Resource::Wood));
    assert_eq!(sim.state.economy.purse, STARTING_PURSE - 4 * material_price(Resource::Wood));
    assert!(c.outcome.ends_with("and were paid 4 gold."), "{}", c.outcome);

    // An empty chest pays nothing, and says so.
    sim.state.economy.purse = 0;
    let gold = sim.state.characters[&id].gold;
    sim.state.characters.get_mut(&id).unwrap().carried.add(Resource::Wood, 2, i64::MAX);
    set_intent(
        &mut sim,
        &id,
        IntentAction::WorldWork { job: WorldJob::Deliver, target: "lumber_camp".into(), minutes: None },
    );
    assert!(run_until(&mut sim, TICKS_PER_DAY / 2, delivered) < TICKS_PER_DAY / 2);
    let c = &sim.state.characters[&id];
    assert_eq!(c.gold, gold);
    assert_eq!(sim.state.economy.purse, 0);
    assert!(c.outcome.contains("nobody paid you"), "{}", c.outcome);
}

#[test]
fn the_economy_survives_a_save_and_load() {
    let mut sim = quiet(6);
    stock(&mut sim.state, "market", Resource::Stone, material_reserve("market") + 8);
    sim.state.characters.get_mut("c2").unwrap().inventory.gift = 3;
    sim.state.characters.get_mut("c2").unwrap().jobs = vec![ActivityId::Trade];
    let traded = |state: &SimState| {
        !state.economy.is_default() && state.chronicle.iter().any(|e| e.kind == ChronicleKind::Trade)
    };
    assert!(run_until(&mut sim, 3 * TICKS_PER_DAY, traded) < 3 * TICKS_PER_DAY);
    let json = serde_json::to_string(&sim.state).unwrap();
    assert!(json.contains("\"economy\""));
    let mut resumed = Sim::new(round_trip(&sim.state));
    assert_eq!(resumed.state, sim.state);
    for _ in 0..4000 {
        sim.step();
        resumed.step();
    }
    assert!(resumed.state == sim.state, "a reloaded economy went a different way");
    let reloaded: Economy = serde_json::from_str("{\"purse\":12}").unwrap();
    assert_eq!((reloaded.purse, reloaded.stock.len()), (12, 0));
}
