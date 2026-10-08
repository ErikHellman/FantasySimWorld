//! Prices, wages and what everyone starts with: every number of the coin economy in one place.
//! See `systems::economy` for how coin moves.

use fsw_world::places::{ActivityId, PadKind};

use crate::data::classes::ClassId;
use crate::state::{ItemId, Resource};

/// What the village chest holds when a village is founded (and what an older save is given).
pub const STARTING_PURSE: i64 = 80;

/// The coin a founder or newcomer arrives with, by their trade: a fixed table, not a roll, so the
/// founding draws from the RNG exactly as it did before there was coin.
pub fn starting_gold(class: ClassId) -> i64 {
    match class {
        ClassId::Knight | ClassId::Paladin => 14,
        ClassId::Lorekeeper | ClassId::Mage => 12,
        ClassId::Engineer | ClassId::Cleric => 10,
        ClassId::Rogue | ClassId::Tiefling | ClassId::Witch => 9,
        ClassId::Ranger | ClassId::Scout | ClassId::Barbarian => 8,
        ClassId::Druid | ClassId::Farmer => 6,
    }
}

/// What a thing is worth at the market with nothing like it on the stalls.
pub fn base_price(item: ItemId) -> i64 {
    match item {
        ItemId::Food => 2,
        ItemId::Potion => 6,
        ItemId::Gift => 3,
        ItemId::Trinket => 8,
        ItemId::Relic => 25,
    }
}

/// What the market pays a carrier for a unit of material put down at a stockpile.
pub fn material_price(resource: Resource) -> i64 {
    match resource {
        Resource::Wood => 1,
        Resource::Stone => 1,
        Resource::Ore => 3,
    }
}

/// The market's price for a thing today: the base, a coin more while the stalls have none, a coin
/// less for every six already there, and never below one. Integer arithmetic, so every machine agrees.
pub fn price(item: ItemId, in_stock: i64) -> i64 {
    let scarce = if in_stock <= 0 { 1 } else { 0 };
    (base_price(item) + scarce - in_stock / 6).max(1)
}

/// What the stalls keep back from the passing trader, so villagers can still shop.
pub fn market_float(item: ItemId) -> i64 {
    match item {
        ItemId::Food => 2,
        ItemId::Potion => 1,
        ItemId::Gift => 1,
        ItemId::Trinket | ItemId::Relic => 0,
    }
}

/// Of each material, how much a stockpile keeps back from the trader: enough at the camp and the mine
/// for the village's own building, a little at the market for the overflow. What the chest pays out
/// in wages comes back this way once the piles are full.
pub fn material_reserve(place_id: &str) -> i64 {
    match place_id {
        "market" => 10,
        _ => 30,
    }
}

/// When the passing trader comes through the market, every day.
pub const TRADER_HOUR: f64 = 11.0;

/// The most a carrier is paid for one load put down, however dear what they carried.
pub const MAX_WAGE_PER_DEPOSIT: i64 = 8;

/// What a villager keeps back when selling at a stall: a meal, a potion and a gift of their own.
pub fn keeps(item: ItemId) -> i64 {
    match item {
        ItemId::Food | ItemId::Potion | ItemId::Gift => 1,
        ItemId::Trinket | ItemId::Relic => 0,
    }
}

/// Whether a class's work puts them behind a stall, where the surplus is sold.
pub fn sells_at_market(jobs: &[ActivityId]) -> bool {
    jobs.contains(&ActivityId::Trade)
}

/// How far a villager goes out of their way for something the dead left, and how close they must
/// stand to take it.
pub const LOOT_REACH: f64 = 25.0;
pub const PICK_REACH: f64 = 1.2;
/// Loot nobody takes is gone after this long.
pub const LOOT_DAYS: i64 = 3;
/// How close one of the dead may be before nobody stoops for loot.
pub const LOOT_DANGER: f64 = 30.0;
/// Of a hundred fallen skeletons, how many leave nothing, and how many coins; the rest leave a trinket.
pub const LOOT_NOTHING_OF_100: u32 = 45;
pub const LOOT_COINS_OF_100: u32 = 45;
/// Coins from a skeleton: this many, plus up to four more.
pub const LOOT_COINS_LEAST: i64 = 2;
/// The least the dragon's hoard holds (up to ten more).
pub const DRAGON_HOARD: i64 = 25;

/// How far a villager looks for a neighbour to buy from or sell to.
pub const TRADE_REACH: f64 = 12.0;
/// A seller this hungry keeps a meal back.
pub const SELLER_KEEPS_FOOD_BELOW: f64 = 40.0;

/// What a villager pays the village to make their small cottage bigger.
pub const EXPAND_PRICE: i64 = 40;
/// What each thing for the yard costs its sponsor.
pub fn yard_price(kind: PadKind) -> i64 {
    match kind {
        PadKind::Wagon => 18,
        PadKind::Chest => 20,
        PadKind::Barrel => 10,
        PadKind::Scarecrow => 8,
        PadKind::Haybale => 6,
        PadKind::Fence | PadKind::LogStack | PadKind::Bench | PadKind::Crate | PadKind::Planter => 0,
    }
}
/// The things a villager may pay for in their own yard; the rest are the village's to put up.
pub const YARD_KINDS: [PadKind; 5] =
    [PadKind::Wagon, PadKind::Chest, PadKind::Barrel, PadKind::Scarecrow, PadKind::Haybale];
/// What the chest pays each builder when a sponsored project is done.
pub const BUILD_WAGE: i64 = 3;
