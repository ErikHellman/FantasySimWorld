//! Coin: where it comes from, where it goes. Villagers are paid for what they put down at the
//! stockpiles and for what they sell at the market's stalls, spend it on the stalls' goods, and the
//! chest the market pays from is filled once a day by a passing trader who buys the surplus. Every
//! number is in `data::economy`.
//!
//! Nothing here draws from the RNG. A world that has never traded holds the starting chest and
//! nothing on the stalls, and this module changes nothing in it until someone sells, buys or is paid.

use fsw_core::math2d::dist;
use fsw_core::rng::hash_string;
use fsw_core::time::{TICKS_PER_DAY, TICKS_PER_GAME_MINUTE};
use fsw_core::{Vec2, vec2};

use fsw_world::places::{PadKind, find_place, pads};

use crate::data::activities::ITEM_CAP;
use crate::data::economy::{
    BUILD_WAGE, DRAGON_HOARD, EXPAND_PRICE, LOOT_COINS_LEAST, LOOT_COINS_OF_100, LOOT_DAYS, LOOT_NOTHING_OF_100,
    MAX_WAGE_PER_DEPOSIT, SELLER_KEEPS_FOOD_BELOW, TRADE_REACH, TRADER_HOUR, YARD_KINDS, base_price, keeps,
    market_float, material_price, material_reserve, price, yard_price,
};
use crate::state::{
    Character, ChronicleKind, ItemId, Loot, LootKind, MemoryKind, MonsterKind, Resource, SimState, StimulusKind, Store,
};
use crate::state::{Project, ProjectKind};
use crate::systems::director::tick_at;
use crate::systems::relationships::RUNG_GRUDGE;
use crate::systems::world::STOCKPILES;
use crate::systems::world::{
    CONSTRUCT_WORK, EXPAND_WORK, MAX_PROJECTS, building_id, building_whole, new_project, pad_kind_cost, pad_kind_name,
    pad_taken, project_label,
};
use crate::util::{Ctx, place_name_at};
use crate::util::{adults, is_approachable, rel_or_default};

/// Pays a villager from the chest, as much of `amount` as the chest holds; returns what was paid.
pub fn pay(c: &mut Character, purse: &mut i64, amount: i64) -> i64 {
    let paid = amount.min(*purse).max(0);
    *purse -= paid;
    c.gold += paid;
    paid
}

/// "12 gold", "1 gold".
pub fn describe_gold(amount: i64) -> String {
    format!("{amount} gold")
}

/// What a carrier is paid for materials put down at a stockpile.
pub fn wages_for(put: &Store) -> i64 {
    put.iter().map(|(resource, count)| material_price(resource) * count).sum::<i64>().min(MAX_WAGE_PER_DEPOSIT)
}

/// Pays a carrier for what they have just put down; returns what they got.
pub fn pay_wages(ctx: &mut Ctx, id: &str, put: &Store) -> i64 {
    let wages = wages_for(put);
    if wages <= 0 {
        return 0;
    }
    let mut purse = ctx.state.economy.purse;
    let paid = pay(ctx.c(id), &mut purse, wages);
    ctx.state.economy.purse = purse;
    paid
}

/// Sells a villager's surplus onto the stalls, a thing at a time at the day's price, until the
/// chest cannot pay for more. Returns what was sold and what it fetched.
pub fn sell_to_market(ctx: &mut Ctx, id: &str) -> (Vec<(ItemId, i64)>, i64) {
    let mut sold = Vec::new();
    let mut earned = 0;
    for item in ItemId::ALL {
        let mut count = 0;
        loop {
            let have = ctx.cr(id).inventory.get(item);
            if have <= keeps(item) {
                break;
            }
            let asked = price(item, ctx.state.economy.in_stock(item));
            if ctx.state.economy.purse < asked {
                break;
            }
            *ctx.c(id).inventory.get_mut(item) -= 1;
            ctx.state.economy.stock_up(item, 1);
            ctx.state.economy.purse -= asked;
            ctx.c(id).gold += asked;
            earned += asked;
            count += 1;
        }
        if count > 0 {
            sold.push((item, count));
        }
    }
    (sold, earned)
}

/// Buys one thing off the stalls, if there is one and the buyer can pay; returns the price paid.
pub fn buy_at_market(ctx: &mut Ctx, id: &str, item: ItemId) -> Option<i64> {
    let in_stock = ctx.state.economy.in_stock(item);
    if in_stock <= 0 {
        return None;
    }
    let asked = price(item, in_stock);
    if ctx.cr(id).gold < asked {
        return None;
    }
    ctx.state.economy.take_stock(item, 1);
    ctx.state.economy.purse += asked;
    let c = ctx.c(id);
    c.gold -= asked;
    crate::systems::intents::give(c, item);
    Some(asked)
}

/// "2 food and 1 trinket": a list of goods, in a fixed order.
pub fn describe_goods(goods: &[(ItemId, i64)]) -> String {
    let parts: Vec<String> = goods.iter().map(|&(item, count)| item.label(count)).collect();
    match parts.len() {
        0 => "nothing".into(),
        1 => parts[0].clone(),
        n => format!("{} and {}", parts[..n - 1].join(", "), parts[n - 1]),
    }
}

/// The passing trader: buys everything on the stalls above what they keep back, at the base price,
/// and each pile's materials above its reserve, and leaves the coin in the chest. One chronicle
/// line, and only when something was sold.
fn trader_visit(ctx: &mut Ctx) {
    let mut goods = Vec::new();
    let mut earned = 0;
    for item in ItemId::ALL {
        let surplus = ctx.state.economy.in_stock(item) - market_float(item);
        if surplus > 0 {
            let taken = ctx.state.economy.take_stock(item, surplus);
            earned += taken * base_price(item);
            goods.push((item, taken));
        }
    }
    let mut materials = Store::default();
    for (place_id, _, _) in STOCKPILES {
        let Some(pile) = ctx.state.stockpiles.get_mut(place_id) else { continue };
        for resource in Resource::ALL {
            let surplus = pile.get(resource) - material_reserve(place_id);
            if surplus > 0 {
                let taken = pile.take(resource, surplus);
                earned += taken * material_price(resource);
                materials.add(resource, taken, i64::MAX);
            }
        }
        if pile.is_empty() {
            ctx.state.stockpiles.shift_remove(place_id);
        }
    }
    if earned == 0 {
        return;
    }
    ctx.state.economy.purse += earned;
    let bought = match (goods.is_empty(), materials.is_empty()) {
        (false, false) => format!("{} and {}", describe_goods(&goods), materials.describe()),
        (false, true) => describe_goods(&goods),
        _ => materials.describe(),
    };
    let line = format!("A passing trader bought {bought} at the market for {}.", describe_gold(earned));
    ctx.chronicle(ChronicleKind::Trade, line, Vec::new());
}

/// A deal between two villagers, as one of them sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeOption {
    pub other_id: String,
    pub item: ItemId,
    /// buying from them, else selling to them
    pub buy: bool,
    pub price: i64,
}

/// What `seller` would part with: everything but a meal when hungry.
fn seller_keeps(seller: &Character, item: ItemId) -> i64 {
    if item == ItemId::Food && seller.needs.hunger < SELLER_KEEPS_FOOD_BELOW { 1 } else { 0 }
}

/// Whether `buyer` may buy `item` from `seller` right now, and for how much; otherwise why not.
pub fn trade_offer(state: &SimState, buyer: &Character, seller: &Character, item: ItemId) -> Result<i64, &'static str> {
    if buyer.is_child() || seller.is_child() {
        return Err("children do not trade");
    }
    if seller.inventory.get(item) <= seller_keeps(seller, item) {
        return Err("nothing to spare");
    }
    if buyer.inventory.get(item) >= ITEM_CAP {
        return Err("no room in the pack");
    }
    let asked = price(item, state.economy.in_stock(item));
    if buyer.gold < asked {
        return Err("not enough coin");
    }
    if rel_or_default(seller, &buyer.id).grievance >= RUNG_GRUDGE {
        return Err("a grudge");
    }
    Ok(asked)
}

/// Whether `other` has a use for `item`: food when short of it, a potion when a fighter without, a
/// valuable when there is room for it, never a gift.
fn wants(other: &Character, item: ItemId) -> bool {
    match item {
        ItemId::Food => other.inventory.food < 2,
        ItemId::Potion => other.inventory.potion == 0 && other.profile.bravery >= 0.6,
        ItemId::Gift => false,
        ItemId::Trinket | ItemId::Relic => other.inventory.get(item) < ITEM_CAP,
    }
}

/// The deals open to `c` with the neighbours within reach, nearest neighbour first: what they could buy
/// that they have a use for, and what they could sell that the other has a use for.
pub fn trade_options(state: &SimState, c: &Character) -> Vec<TradeOption> {
    let mut options = Vec::new();
    if c.is_child() {
        return options;
    }
    let mut near: Vec<(f64, &Character)> = adults(state)
        .filter(|o| o.id != c.id && is_approachable(o) && dist(o.pos, c.pos) <= TRADE_REACH)
        .map(|o| (dist(o.pos, c.pos), o))
        .collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.id.cmp(&b.1.id)));
    for (_, other) in near {
        for item in ItemId::ALL {
            if wants(c, item)
                && let Ok(price) = trade_offer(state, c, other, item)
            {
                options.push(TradeOption { other_id: other.id.clone(), item, buy: true, price });
            }
            if wants(other, item)
                && let Ok(price) = trade_offer(state, other, c, item)
            {
                options.push(TradeOption { other_id: other.id.clone(), item, buy: false, price });
            }
        }
    }
    options
}

/// Something a villager may pay the village to build for them: a new room for their cottage, or a
/// thing for a pad in their yard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Commission {
    Expand { home_id: String },
    Yard { pad_id: String, kind: PadKind },
}

impl Commission {
    /// `expand` or `yard:<pad id>:<kind>`, as an intent carries it.
    pub fn key(&self) -> String {
        match self {
            Self::Expand { .. } => "expand".into(),
            Self::Yard { pad_id, kind } => format!("yard:{pad_id}:{}", kind.as_str()),
        }
    }

    pub fn parse(key: &str, home_id: &str) -> Option<Commission> {
        if key == "expand" {
            return Some(Self::Expand { home_id: home_id.to_owned() });
        }
        let rest = key.strip_prefix("yard:")?;
        let (pad_id, kind) = rest.rsplit_once(':')?;
        Some(Self::Yard { pad_id: pad_id.to_owned(), kind: PadKind::parse(kind)? })
    }

    pub fn price(&self) -> i64 {
        match self {
            Self::Expand { .. } => EXPAND_PRICE,
            Self::Yard { kind, .. } => yard_price(*kind),
        }
    }

    /// "a new room for your cottage", "a wagon in your yard".
    pub fn label(&self) -> String {
        match self {
            Self::Expand { .. } => "a new room for your cottage".into(),
            Self::Yard { kind, .. } => format!("{} in your yard", pad_kind_name(*kind)),
        }
    }
}

/// What `c` could pay for right now, dearest first: a new room when their small cottage stands whole
/// and nobody is building on it, then each thing for a free pad by their home they have the coin for.
pub fn affordable(state: &SimState, c: &Character) -> Vec<Commission> {
    let mut options = Vec::new();
    if c.is_child() {
        return options;
    }
    let Some(home) = find_place(&c.home_id) else { return options };
    let small = home.models.first().is_some_and(|m| m.model.contains("home_A"));
    let untouched = !state.objects.contains_key(&building_id(&home.id))
        && !state.houses.contains_key(&home.id)
        && building_whole(state, &home.id)
        && !state.projects.iter().any(|p| p.target == building_id(&home.id));
    if small && untouched && c.gold >= EXPAND_PRICE {
        options.push(Commission::Expand { home_id: home.id.clone() });
    }
    for pad in pads() {
        if !pad.id.starts_with(&format!("{}/", home.id)) || pad_taken(state, &pad.id) {
            continue;
        }
        for kind in YARD_KINDS {
            if pad.kinds.contains(&kind) && c.gold >= yard_price(kind) {
                options.push(Commission::Yard { pad_id: pad.id.clone(), kind });
            }
        }
    }
    options
}

/// Pays for a commission and puts it on the village's list, if it still can be afforded and the
/// village has hands to spare; returns the project, or why not.
pub fn commission(ctx: &mut Ctx, id: &str, choice: &Commission) -> Result<String, &'static str> {
    if !affordable(ctx.state, ctx.cr(id)).contains(choice) {
        return Err("cannot afford that, or it is taken");
    }
    if ctx.state.projects.len() >= MAX_PROJECTS {
        return Err("the village has its hands full");
    }
    let price = choice.price();
    let (kind, target, cost, need, pad_kind) = match choice {
        Commission::Expand { home_id } => {
            let mut cost = Store::default();
            cost.add(Resource::Wood, 20, i64::MAX);
            cost.add(Resource::Stone, 10, i64::MAX);
            (ProjectKind::Expand, building_id(home_id), cost, EXPAND_WORK, None)
        }
        Commission::Yard { pad_id, kind } => {
            (ProjectKind::Construct, pad_id.clone(), pad_kind_cost(*kind), CONSTRUCT_WORK, Some(*kind))
        }
    };
    let mut project = new_project(ctx.state, kind, target, cost, need);
    project.sponsor = Some(id.to_owned());
    project.price = price;
    project.pad_kind = pad_kind;
    ctx.c(id).gold -= price;
    ctx.state.economy.purse += price;
    let label = project_label(ctx.state, &project);
    let project_id = project.id.clone();
    ctx.chronicle(ChronicleKind::Trade, format!("The village takes on {label}."), vec![id.to_owned()]);
    ctx.remember(
        id,
        format!("Paid {} for {}.", describe_gold(price), choice.label().replace("your", "my")),
        5.0,
        MemoryKind::Event,
        Vec::new(),
    );
    ctx.state.projects.push(project);
    Ok(project_id)
}

/// Once a day, when the village looks over what wants building: the first villager (in the roster's
/// order) with something they can pay for pays for one of their options, chosen by their id and the
/// day so every machine agrees. `taken` is how many of the day's slots are already spoken for.
pub fn sponsored_project(ctx: &mut Ctx, taken: usize) {
    if ctx.state.projects.len() + taken >= MAX_PROJECTS {
        return;
    }
    let day = ctx.time.day;
    let ids: Vec<String> = ctx.state.order.clone();
    for id in ids {
        let options = affordable(ctx.state, ctx.cr(&id));
        if options.is_empty() {
            continue;
        }
        let pick = (hash_string(&id) as usize + day as usize) % options.len();
        let _ = commission(ctx, &id, &options[pick]);
        return;
    }
}

/// The chest pays each builder of a sponsored project when it is done.
pub fn pay_builders(ctx: &mut Ctx, project: &Project) {
    if project.sponsor.is_none() {
        return;
    }
    for helper in &project.helpers {
        if ctx.alive(helper) {
            let mut purse = ctx.state.economy.purse;
            pay(ctx.c(helper), &mut purse, BUILD_WAGE);
            ctx.state.economy.purse = purse;
        }
    }
}

/// One of the dead has fallen: what it leaves is fixed by the monster and the tick, so every machine
/// agrees and the RNG is not drawn. The necromancer always leaves a relic; the dragon, its hoard. The
/// killer is told.
pub fn drop_loot(ctx: &mut Ctx, monster_id: &str, kind: MonsterKind, pos: Vec2, killer: &str) {
    let tick = ctx.tick();
    let roll = hash_string(&format!("{monster_id}@{tick}"));
    if kind == MonsterKind::Dragon {
        let coins = LootKind::Coins { count: DRAGON_HOARD + i64::from(roll % 11) };
        for (left, offset) in [(LootKind::Relic, 1.2), (coins, -1.2)] {
            let id = ctx.state.new_id("l:");
            ctx.state.loot.insert(id, Loot { kind: left, pos: vec2(pos.x + offset, pos.z), since_tick: tick });
        }
        let text = format!("The dragon's hoard spilled where it fell, at {}: {}.", place_name_at(pos), coins.label());
        ctx.remember(killer, text.clone(), 5.0, MemoryKind::Event, Vec::new());
        ctx.stimulate(killer, StimulusKind::Loot, text, 0, None);
        return;
    }
    let left = if kind == MonsterKind::Necromancer {
        LootKind::Relic
    } else {
        match roll % 100 {
            n if n < LOOT_NOTHING_OF_100 => return,
            n if n < LOOT_NOTHING_OF_100 + LOOT_COINS_OF_100 => {
                LootKind::Coins { count: LOOT_COINS_LEAST + i64::from((roll / 100) % 5) }
            }
            _ => LootKind::Trinket,
        }
    };
    let id = ctx.state.new_id("l:");
    ctx.state.loot.insert(id, Loot { kind: left, pos, since_tick: tick });
    let text = format!("The {} left {} where it fell, at {}.", kind.def().label, left.label(), place_name_at(pos));
    ctx.remember(killer, text.clone(), 3.0, MemoryKind::Event, Vec::new());
    ctx.stimulate(killer, StimulusKind::Loot, text, 0, None);
}

/// "4 coins near the village green": loot as it is named to a villager.
pub fn loot_label(loot: &Loot) -> String {
    format!("{} near {}", loot.kind.label(), place_name_at(loot.pos))
}

/// The loot within `reach` of `pos`, nearest first (then by id, so every machine agrees).
pub fn loot_near(state: &SimState, pos: Vec2, reach: f64) -> Vec<(f64, String)> {
    let mut near: Vec<(f64, String)> =
        state.loot.iter().map(|(id, loot)| (dist(pos, loot.pos), id.clone())).filter(|(d, _)| *d <= reach).collect();
    near.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    near
}

/// Whether someone is already on their way to this loot.
pub fn loot_claimed(state: &SimState, loot_id: &str, by_other_than: &str) -> bool {
    state.characters.values().any(|c| {
        c.id != by_other_than
            && c.intent
                .as_ref()
                .is_some_and(|i| matches!(&i.action, crate::state::IntentAction::PickUp { loot_id: l } if l == loot_id))
    })
}

/// Runs the economy's clock: the trader's daily visit, and loot nobody took blowing away.
pub fn update_economy(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE != 0 {
        return;
    }
    if tick == tick_at(ctx.time.day, TRADER_HOUR) {
        trader_visit(ctx);
    }
    if !ctx.state.loot.is_empty() {
        let old: Vec<String> = ctx
            .state
            .loot
            .iter()
            .filter(|(_, loot)| tick - loot.since_tick >= LOOT_DAYS * TICKS_PER_DAY)
            .map(|(id, _)| id.clone())
            .collect();
        for id in old {
            ctx.state.loot.shift_remove(&id);
        }
    }
}
