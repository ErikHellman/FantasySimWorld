//! Coin: where it comes from, where it goes. Villagers are paid for what they put down at the
//! stockpiles and for what they sell at the market's stalls, spend it on the stalls' goods, and the
//! chest the market pays from is filled once a day by a passing trader who buys the surplus. Every
//! number is in `data::economy`.
//!
//! Nothing here draws from the RNG. A world that has never traded holds the starting chest and
//! nothing on the stalls, and this module changes nothing in it until someone sells, buys or is paid.

use fsw_core::time::TICKS_PER_GAME_MINUTE;

use crate::data::economy::{
    MAX_WAGE_PER_DEPOSIT, TRADER_HOUR, base_price, keeps, market_float, material_price, material_reserve, price,
};
use crate::state::{Character, ChronicleKind, ItemId, Resource, Store};
use crate::systems::director::tick_at;
use crate::systems::world::STOCKPILES;
use crate::util::Ctx;

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

/// Runs the economy's clock: the trader's daily visit.
pub fn update_economy(ctx: &mut Ctx) {
    let tick = ctx.tick();
    if tick % TICKS_PER_GAME_MINUTE != 0 {
        return;
    }
    if tick == tick_at(ctx.time.day, TRADER_HOUR) {
        trader_visit(ctx);
    }
}
