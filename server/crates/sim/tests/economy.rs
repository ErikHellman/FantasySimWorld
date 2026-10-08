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

/// Takes everyone's coin, so nobody pays the village for anything while the test looks elsewhere.
fn penniless(sim: &mut Sim) {
    for c in sim.state.characters.values_mut() {
        c.gold = 0;
    }
}

/// The chronicle's lines about the passing trader (paying for building is a `Trade` line too).
fn trade_lines(state: &SimState) -> usize {
    state.chronicle.iter().filter(|e| e.kind == ChronicleKind::Trade && e.text.starts_with("A passing trader")).count()
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
    penniless(&mut sim);
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
    penniless(&mut sim);
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

mod loot {
    //! What the dead leave behind, and who stoops for it.

    use super::*;
    use fsw_core::vec2;
    use fsw_sim::data::economy::LOOT_DAYS;
    use fsw_sim::state::{Loot, LootKind, MonsterKind};
    use fsw_sim::systems::combat::damage_monster;
    use fsw_sim::systems::monsters::spawn_monster;
    use fsw_sim::util::Ctx;
    use fsw_world::places::get_place;

    fn with<R>(sim: &mut Sim, f: impl FnOnce(&mut Ctx) -> R) -> R {
        let mut events = Vec::new();
        let mut ctx = Ctx::new(&mut sim.state, &mut events);
        f(&mut ctx)
    }

    fn slay(sim: &mut Sim, kind: MonsterKind) -> Vec<LootKind> {
        let pos = get_place("green").origin;
        with(sim, |ctx| {
            let id = spawn_monster(ctx, kind, pos, 0.0);
            damage_monster(ctx, &id, 10_000, "c1");
        });
        sim.state.loot.values().map(|l| l.kind).collect()
    }

    #[test]
    fn a_slain_skeleton_may_leave_loot_and_the_necromancer_always_a_relic() {
        let mut sim = quiet(7);
        let rng = sim.state.rng;
        let mut kinds = Vec::new();
        for _ in 0..40 {
            kinds = slay(&mut sim, MonsterKind::SkeletonMinion);
        }
        assert_eq!(sim.state.rng, rng, "what the dead leave is fixed, not rolled");
        assert!(kinds.len() < 40, "not every skeleton leaves something");
        assert!(kinds.iter().any(|k| matches!(k, LootKind::Coins { .. })), "{kinds:?}");
        for kind in &kinds {
            if let LootKind::Coins { count } = kind {
                assert!((2..=6).contains(count), "{count}");
            }
        }
        let before = sim.state.loot.len();
        let kinds = slay(&mut sim, MonsterKind::Necromancer);
        assert_eq!(kinds.len(), before + 1);
        assert_eq!(kinds[before], LootKind::Relic);
        // The killer heard of it.
        let killer = &sim.state.characters["c1"];
        assert!(
            killer.memories.iter().any(|m| m.text.contains("left a relic where it fell")),
            "{:?}",
            killer.memories.last()
        );
        assert!(killer.stimuli.iter().any(|s| s.text.contains("relic")));
        // Two fresh worlds agree.
        let mut again = quiet(7);
        for _ in 0..40 {
            slay(&mut again, MonsterKind::SkeletonMinion);
        }
        slay(&mut again, MonsterKind::Necromancer);
        assert_eq!(again.state.loot, sim.state.loot);
    }

    fn drop_near(sim: &mut Sim, id: &str, kind: LootKind) -> String {
        let c = &sim.state.characters[id];
        let pos = sim.state.nav.nearest_walkable(vec2(c.pos.x + 3.0, c.pos.z + 1.0), 4).unwrap();
        let loot_id = sim.state.new_id("l:");
        let tick = sim.state.tick;
        sim.state.loot.insert(loot_id.clone(), Loot { kind, pos, since_tick: tick });
        loot_id
    }

    #[test]
    fn coins_on_the_ground_are_picked_up_by_day() {
        let mut sim = quiet(8);
        // Mid-morning, with everyone up and about.
        run_until(&mut sim, TICKS_PER_DAY, |s| fsw_core::time::game_time(s.tick).hour >= 9.0);
        let gold = sim.state.characters["c1"].gold;
        let coins = drop_near(&mut sim, "c1", LootKind::Coins { count: 4 });
        let trinket = drop_near(&mut sim, "c1", LootKind::Trinket);
        let taken = |s: &SimState| !s.loot.contains_key(&coins) && !s.loot.contains_key(&trinket);
        assert!(run_until(&mut sim, TICKS_PER_DAY / 4, taken) < TICKS_PER_DAY / 4, "nobody stooped for it");
        let picked: Vec<&fsw_sim::state::Character> =
            living(&sim.state).filter(|c| c.memories.iter().any(|m| m.text.starts_with("Picked up 4 coins"))).collect();
        assert_eq!(picked.len(), 1);
        assert!(picked[0].gold >= 4, "{}", picked[0].gold);
        assert!(living(&sim.state).any(|c| c.inventory.trinket == 1));
        if picked[0].id == "c1" {
            assert!(picked[0].gold >= gold + 4);
        }
    }

    #[test]
    fn nobody_stoops_for_loot_while_the_dead_are_near_or_by_night() {
        let mut sim = quiet(9);
        run_until(&mut sim, TICKS_PER_DAY, |s| fsw_core::time::game_time(s.tick).hour >= 9.0);
        let coins = drop_near(&mut sim, "c1", LootKind::Coins { count: 3 });
        let at = sim.state.loot[&coins].pos;
        let post = vec2(at.x + 6.0, at.z);
        let warrior = with(&mut sim, |ctx| spawn_monster(ctx, MonsterKind::SkeletonWarrior, post, 0.0));
        // The warrior is held at its post all morning: unkillable, and not one to crumble or lie low.
        for _ in 0..600 {
            let m = sim.state.monsters.get_mut(&warrior).unwrap();
            m.hp = 10_000.0;
            m.lasting = true;
            m.pos = post;
            sim.step();
        }
        assert!(sim.state.loot.contains_key(&coins), "someone stooped with the dead six paces off");
        assert!(
            !living(&sim.state)
                .any(|c| matches!(c.intent.as_ref().map(|i| &i.action), Some(IntentAction::PickUp { .. })))
        );
    }

    #[test]
    fn loot_blocks_nothing_and_blows_away_after_three_days() {
        let mut sim = quiet(10);
        let coins = drop_near(&mut sim, "c2", LootKind::Coins { count: 2 });
        assert!(!sim.state.nav.is_live(), "loot is not on the nav grid");
        // Nobody can reach it: it lies in the dark of the first night and is then forgotten by all.
        sim.state.loot[&coins].since_tick -= LOOT_DAYS * TICKS_PER_DAY;
        run_until(&mut sim, 2 * fsw_core::time::TICKS_PER_GAME_MINUTE, |_| false);
        assert!(!sim.state.loot.contains_key(&coins), "three-day-old loot is gone");
    }

    #[test]
    fn loot_survives_a_save_and_load() {
        let mut sim = quiet(11);
        // A step first, so the quiet tuning is in the world and not still queued beside it.
        sim.step();
        slay(&mut sim, MonsterKind::Necromancer);
        drop_near(&mut sim, "c1", LootKind::Coins { count: 5 });
        let json = serde_json::to_string(&sim.state).unwrap();
        assert!(json.contains("\"loot\""));
        let mut resumed = Sim::new(round_trip(&sim.state));
        assert_eq!(resumed.state, sim.state);
        for _ in 0..3000 {
            sim.step();
            resumed.step();
        }
        assert!(resumed.state == sim.state, "reloaded loot went a different way");
    }
}

mod trade {
    //! Buying from a neighbour and selling to one.

    use super::*;
    use fsw_core::vec2;
    use fsw_sim::systems::economy::trade_options;
    use fsw_sim::systems::relationships::RUNG_GRUDGE;
    use fsw_sim::util::rel;

    /// Puts two villagers side by side on the green, in daylight, with nothing else on their minds.
    fn side_by_side(seed: u32) -> Sim {
        let mut sim = quiet(seed);
        run_until(&mut sim, TICKS_PER_DAY, |s| fsw_core::time::game_time(s.tick).hour >= 9.0);
        let spot = fsw_world::places::get_place("green").origin;
        for (id, dx) in [("c1", -1.0), ("c2", 1.0)] {
            let c = sim.state.characters.get_mut(id).unwrap();
            c.pos = vec2(spot.x + dx, spot.z);
            c.inside = None;
            c.intent = None;
            c.queue.clear();
        }
        sim
    }

    #[test]
    fn a_hungry_neighbour_with_coin_buys_a_meal_rather_than_walk_to_the_tavern() {
        let mut sim = side_by_side(12);
        {
            let buyer = sim.state.characters.get_mut("c1").unwrap();
            buyer.inventory.food = 0;
            buyer.needs.hunger = 20.0;
            buyer.gold = 5;
            let seller = sim.state.characters.get_mut("c2").unwrap();
            seller.inventory.food = 3;
            seller.needs.hunger = 80.0;
            seller.gold = 0;
        }
        let options = trade_options(&sim.state, &sim.state.characters["c1"]);
        assert!(options.iter().any(|o| o.buy && o.item == ItemId::Food && o.other_id == "c2"), "{options:?}");
        let asked = price(ItemId::Food, 0);
        let bought = |s: &SimState| s.characters["c2"].gold > 0;
        assert!(run_until(&mut sim, TICKS_PER_DAY / 6, bought) < TICKS_PER_DAY / 6, "no deal was struck");
        let (buyer, seller) = (&sim.state.characters["c1"], &sim.state.characters["c2"]);
        assert_eq!((buyer.gold, seller.gold), (5 - asked, asked));
        assert_eq!(seller.inventory.food, 2);
        assert!(buyer.needs.hunger > 40.0, "the meal was eaten on the spot: {}", buyer.needs.hunger);
        assert!(
            buyer.memories.iter().any(|m| m.text.starts_with("Bought some food from")),
            "{:?}",
            buyer.memories.last()
        );
        assert!(seller.memories.iter().any(|m| m.text.starts_with("Sold ")), "{:?}", seller.memories.last());
        assert!(buyer.outcome.contains("and ate it"), "{}", buyer.outcome);
    }

    #[test]
    fn a_seller_with_a_grudge_or_nothing_to_spare_will_not_sell() {
        let mut sim = side_by_side(13);
        {
            let buyer = sim.state.characters.get_mut("c1").unwrap();
            buyer.inventory.food = 0;
            buyer.gold = 9;
            let seller = sim.state.characters.get_mut("c2").unwrap();
            seller.inventory.food = 3;
            rel(seller, "c1").grievance = RUNG_GRUDGE;
        }
        assert!(!trade_options(&sim.state, &sim.state.characters["c1"]).iter().any(|o| o.other_id == "c2"));
        set_intent(&mut sim, "c1", IntentAction::Trade { target_id: "c2".into(), item: ItemId::Food, buy: true });
        let settled = |s: &SimState| s.characters["c1"].outcome.contains("would not sell");
        assert!(run_until(&mut sim, TICKS_PER_DAY / 6, settled) < TICKS_PER_DAY / 6);
        assert!(sim.state.characters["c1"].outcome.ends_with("a grudge."), "{}", sim.state.characters["c1"].outcome);
        assert_eq!(sim.state.characters["c1"].gold, 9);
        assert_eq!(sim.state.characters["c2"].inventory.food, 3);

        // A hungry seller keeps their last meal back.
        let seller = sim.state.characters.get_mut("c2").unwrap();
        rel(seller, "c1").grievance = 0.0;
        seller.inventory.food = 1;
        seller.needs.hunger = 20.0;
        set_intent(&mut sim, "c1", IntentAction::Trade { target_id: "c2".into(), item: ItemId::Food, buy: true });
        sim.state.characters.get_mut("c1").unwrap().outcome.clear();
        assert!(run_until(&mut sim, TICKS_PER_DAY / 6, settled) < TICKS_PER_DAY / 6);
        assert!(
            sim.state.characters["c1"].outcome.ends_with("nothing to spare."),
            "{}",
            sim.state.characters["c1"].outcome
        );
    }

    #[test]
    fn a_valuable_is_sold_to_a_neighbour_with_the_coin() {
        let mut sim = side_by_side(14);
        {
            let seller = sim.state.characters.get_mut("c1").unwrap();
            seller.inventory.trinket = 1;
            seller.gold = 0;
            let buyer = sim.state.characters.get_mut("c2").unwrap();
            buyer.gold = 20;
        }
        let asked = price(ItemId::Trinket, 0);
        let options = trade_options(&sim.state, &sim.state.characters["c1"]);
        assert!(options.iter().any(|o| !o.buy && o.item == ItemId::Trinket && o.price == asked), "{options:?}");
        set_intent(&mut sim, "c1", IntentAction::Trade { target_id: "c2".into(), item: ItemId::Trinket, buy: false });
        let sold = |s: &SimState| s.characters["c1"].gold > 0;
        assert!(run_until(&mut sim, TICKS_PER_DAY / 6, sold) < TICKS_PER_DAY / 6);
        assert_eq!(sim.state.characters["c1"].gold, asked);
        assert_eq!(sim.state.characters["c2"].inventory.trinket, 1);
        assert_eq!(sim.state.characters["c2"].gold, 20 - asked);
    }

    #[test]
    fn nobody_trades_with_a_child() {
        let mut sim = quiet(15);
        let child = living(&sim.state).find(|c| c.is_child()).map(|c| c.id.clone()).expect("a child");
        sim.state.characters.get_mut(&child).unwrap().inventory.food = 3;
        sim.state.characters.get_mut("c1").unwrap().gold = 9;
        set_intent(&mut sim, "c1", IntentAction::Trade { target_id: child.clone(), item: ItemId::Food, buy: true });
        sim.step();
        sim.step();
        let c1 = &sim.state.characters["c1"];
        assert!(!matches!(c1.intent.as_ref().map(|i| &i.action), Some(IntentAction::Trade { .. })), "{:?}", c1.intent);
        assert!(trade_options(&sim.state, &sim.state.characters[&child]).is_empty());
    }
}

mod sponsor {
    //! Paying the village for a bigger house, or something for the yard.

    use super::*;
    use fsw_sim::data::economy::{BUILD_WAGE, EXPAND_PRICE, YARD_KINDS, yard_price};
    use fsw_sim::state::{BuildingStage, ProjectKind, PropStage, WorldObject};
    use fsw_sim::systems::economy::{Commission, affordable};
    use fsw_sim::systems::world::{beds_of, building_id, project};
    use fsw_world::places::{PadKind, find_pad, find_place, pads};

    /// Stocks the stores so the builders want for nothing, and keeps the dead away.
    fn stocked(seed: u32) -> Sim {
        let mut sim = quiet(seed);
        stock(&mut sim.state, "lumber_camp", Resource::Wood, 40);
        stock(&mut sim.state, "mine", Resource::Stone, 20);
        stock(&mut sim.state, "mine", Resource::Ore, 4);
        // Nobody else has the coin to get in first.
        for c in sim.state.characters.values_mut() {
            c.gold = 0;
        }
        sim
    }

    fn small_cottage_dweller(state: &SimState) -> String {
        living(state)
            .find(|c| !c.is_child() && find_place(&c.home_id).unwrap().models[0].model.contains("home_A"))
            .map(|c| c.id.clone())
            .expect("someone in a small cottage")
    }

    #[test]
    fn a_villager_with_enough_gold_pays_for_a_bigger_cottage() {
        let mut sim = stocked(16);
        let id = small_cottage_dweller(&sim.state);
        let home = sim.state.characters[&id].home_id.clone();
        sim.state.characters.get_mut(&id).unwrap().gold = EXPAND_PRICE + 20;
        let options = affordable(&sim.state, &sim.state.characters[&id]);
        assert_eq!(options.first(), Some(&Commission::Expand { home_id: home.clone() }), "{options:?}");
        assert!(options.len() > 1, "and things for the yard: {options:?}");
        let purse = sim.state.economy.purse;
        let sponsored = |s: &SimState| s.projects.iter().any(|p| p.sponsor.as_deref() == Some(id.as_str()));
        assert!(run_until(&mut sim, 2 * TICKS_PER_DAY, sponsored) < 2 * TICKS_PER_DAY, "nothing was paid for");
        let p = sim.state.projects.iter().find(|p| p.sponsor.is_some()).unwrap().clone();
        assert!(p.price > 0);
        assert_eq!(sim.state.characters[&id].gold, EXPAND_PRICE + 20 - p.price);
        assert!(sim.state.economy.purse >= purse + p.price - 20, "the chest took the coin");
        let line = sim
            .state
            .chronicle
            .iter()
            .find(|e| e.kind == ChronicleKind::Trade && e.text.contains("paid for by"))
            .unwrap();
        assert!(line.text.starts_with("The village takes on "), "{}", line.text);
        assert!(sim.state.characters[&id].memories.iter().any(|m| m.text.starts_with("Paid ")));
        // It is built, and the builders are paid when it is done.
        let before: i64 = living(&sim.state).map(|c| c.gold).sum();
        let done = |s: &SimState| project(s, &p.id).is_none();
        assert!(run_until(&mut sim, 10 * TICKS_PER_DAY, done) < 10 * TICKS_PER_DAY, "never finished");
        match p.kind {
            ProjectKind::Expand => {
                assert!(matches!(
                    sim.state.objects.get(&building_id(&home)),
                    Some(WorldObject::Building { stage: BuildingStage::Expanded, .. })
                ));
                assert_eq!(beds_of(&sim.state, &home), 3);
            }
            ProjectKind::Construct => {
                let built = sim.state.objects.values().find_map(|o| match o {
                    WorldObject::Built { stage: PropStage::Whole, spec, .. }
                        if spec.pad.as_deref() == Some(p.target.as_str()) =>
                    {
                        Some(spec.clone())
                    }
                    _ => None,
                });
                let spec = built.expect("the thing stands on the pad");
                assert!(p.target.starts_with(&format!("{home}/")), "on the sponsor's own pad: {}", p.target);
                let (bundle, model) = match p.pad_kind.unwrap() {
                    PadKind::Wagon => ("halloween", "wagon"),
                    PadKind::Haybale => ("halloween", "haybale"),
                    PadKind::Scarecrow => ("halloween", "scarecrow"),
                    PadKind::Chest => ("dungeon", "chest_gold"),
                    PadKind::Barrel => ("dungeon", "barrel_large"),
                    other => panic!("not a yard thing: {other:?}"),
                };
                assert_eq!((spec.bundle.as_str(), spec.model.as_str()), (bundle, model));
            }
            other => panic!("{other:?}"),
        }
        let after: i64 = living(&sim.state).map(|c| c.gold).sum();
        assert!(after >= before, "the builders were paid their {BUILD_WAGE}: {before} -> {after}");
    }

    #[test]
    fn a_yard_thing_is_built_on_the_sponsors_own_pad_for_its_price() {
        let mut sim = stocked(17);
        let id = small_cottage_dweller(&sim.state);
        let home = sim.state.characters[&id].home_id.clone();
        // Enough for the yard, not for a room.
        sim.state.characters.get_mut(&id).unwrap().gold = EXPAND_PRICE - 1;
        let options = affordable(&sim.state, &sim.state.characters[&id]);
        assert!(!options.iter().any(|o| matches!(o, Commission::Expand { .. })));
        assert!(
            options
                .iter()
                .all(|o| matches!(o, Commission::Yard { pad_id, .. } if pad_id.starts_with(&format!("{home}/")))),
            "{options:?}"
        );
        let pad = pads().into_iter().find(|p| p.id.starts_with(&format!("{home}/"))).unwrap();
        let kind = YARD_KINDS.into_iter().find(|k| pad.kinds.contains(k)).unwrap();
        let choice = Commission::Yard { pad_id: pad.id.clone(), kind };
        assert_eq!(Commission::parse(&choice.key(), &home), Some(choice.clone()));
        set_intent(&mut sim, &id, IntentAction::Commission { what: choice.key() });
        let paid = |s: &SimState| s.projects.iter().any(|p| p.sponsor.as_deref() == Some(id.as_str()));
        assert!(run_until(&mut sim, TICKS_PER_DAY / 2, paid) < TICKS_PER_DAY / 2, "never got to the market");
        let p = sim.state.projects.iter().find(|p| p.sponsor.is_some()).unwrap().clone();
        assert_eq!(
            (p.kind, p.target.as_str(), p.pad_kind, p.price),
            (ProjectKind::Construct, pad.id.as_str(), Some(kind), yard_price(kind))
        );
        assert_eq!(sim.state.characters[&id].gold, EXPAND_PRICE - 1 - yard_price(kind));
        assert!(sim.state.characters[&id].outcome.starts_with("You paid "), "{}", sim.state.characters[&id].outcome);
        assert!(find_pad(&pad.id).is_some());
        // A second order for the same pad is refused: it is taken.
        assert!(!affordable(&sim.state, &sim.state.characters[&id]).contains(&choice));
    }

    #[test]
    fn the_model_cannot_order_what_the_villager_cannot_afford_nor_when_the_village_is_busy() {
        let mut sim = stocked(18);
        let id = small_cottage_dweller(&sim.state);
        let home = sim.state.characters[&id].home_id.clone();
        sim.state.characters.get_mut(&id).unwrap().gold = 1;
        set_intent(&mut sim, &id, IntentAction::Commission { what: "expand".into() });
        sim.step();
        sim.step();
        assert!(!matches!(
            sim.state.characters[&id].intent.as_ref().map(|i| &i.action),
            Some(IntentAction::Commission { .. })
        ));
        assert!(sim.state.projects.iter().all(|p| p.sponsor.is_none()));
        // With the coin but the village's hands full, the daily look passes the sponsor over.
        sim.state.characters.get_mut(&id).unwrap().gold = EXPAND_PRICE;
        for n in 0..fsw_sim::systems::world::MAX_PROJECTS {
            let target = format!("b:home_{}", 7 + n);
            let mut cost = fsw_sim::state::Store::default();
            cost.add(Resource::Wood, 1, i64::MAX);
            let pid = sim.state.new_id("j:");
            sim.state.projects.push(fsw_sim::state::Project {
                id: pid,
                kind: ProjectKind::Expand,
                target,
                cost: cost.clone(),
                delivered: cost,
                need: 1_000_000.0,
                work: 0.0,
                since_tick: sim.state.tick,
                helpers: Vec::new(),
                sponsor: None,
                price: 0,
                pad_kind: None,
            });
        }
        run_until(&mut sim, 2 * TICKS_PER_DAY, |_| false);
        assert!(
            sim.state.projects.iter().all(|p| p.sponsor.is_none()),
            "{:?}",
            sim.state.projects.iter().map(|p| &p.target).collect::<Vec<_>>()
        );
        assert!(sim.state.characters[&id].gold >= EXPAND_PRICE, "nothing was paid; wages may have come in");
        let _ = home;
    }

    #[test]
    fn every_yard_thing_has_a_model_in_the_manifest() {
        for kind in YARD_KINDS {
            let (bundle, model) = match kind {
                PadKind::Wagon => ("halloween", "wagon"),
                PadKind::Haybale => ("halloween", "haybale"),
                PadKind::Scarecrow => ("halloween", "scarecrow"),
                PadKind::Chest => ("dungeon", "chest_gold"),
                PadKind::Barrel => ("dungeon", "barrel_large"),
                other => panic!("not a yard thing: {other:?}"),
            };
            assert!(fsw_world::assets::model_bounds(bundle, model).is_some(), "{bundle}/{model}");
            assert!(yard_price(kind) > 0);
        }
    }
}
