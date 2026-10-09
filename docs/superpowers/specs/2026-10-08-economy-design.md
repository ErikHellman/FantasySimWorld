# Coin: earning, trading, loot, and paying for a bigger house

The village had a materials economy and no money: wood, stone and ore were gathered into stockpiles and spent on what the village chose to build, and once the pads were built the stores filled and stayed full. Now there is coin. Villagers are paid for what they put down and sell, spend it on the market's goods and on one another's, pick up what the dead leave behind, and whoever has saved enough pays for a bigger house or something for the yard. A passing trader buys the surplus once a day, so the stores no longer saturate.

Decisions made with the owner: gold enters through monster loot and the trader; everyday needs stay free (a tavern meal and a bed cost nothing, so nobody starves for want of coin); villagers trade at the market and directly with each other; and the model gets `trade`, `pick_up` and `commission` tools, with the prompt fixture changed on purpose.

Everything here runs in the sim, draws nothing new from its RNG, and is saved with the world, so a world replays as it did and older saves load unchanged.

## What happens

### Earning and spending

- **Everyone has a purse.** A founder or newcomer arrives with a few coins by their trade (6 to 14; a knight or a paladin the most, a farmer or a druid the least); a child has none. The village has a chest, which starts with 80 gold.
- **Carriers are paid.** Whoever puts materials down at a stockpile is paid from the chest: a coin a log or a block, three a lump of ore, at most eight a load. An empty chest pays nothing, and the carrier is told so.
- **Stall keepers sell.** A shift at the market stall ends with the surplus of the keeper's pack sold onto the stalls: everything but one meal, one potion and one gift, and every valuable. The chest pays the day's price, a thing at a time, until it cannot.
- **Prices move.** A thing's price is its base (food 2, a potion 6, a gift 3, a trinket 8, a relic 25), a coin more while the stalls have none of it, a coin less for every six already there, and never below one.
- **Shopping costs coin.** Browsing the market buys one thing off the stalls, if the stalls have it and the shopper can pay: food when the pack is short of it, a potion for a fighter without one, else a small gift. Bare stalls, or an empty purse, buy nothing.
- **The passing trader** comes through the market at eleven each day and buys everything on the stalls above what they keep back (two food, a potion and a gift), and each pile's wood, stone and ore above its reserve (thirty of each at the camp and the mine, for the village's own building; ten at the market), at the base price, and leaves the coin in the chest. What the chest pays out in wages comes back this way once the piles are full. One chronicle line a day, and only when something was sold.

### Loot

- When one of the dead falls, it may leave something where it fell: nothing, most often; a few coins; now and then a trinket. The necromancer always leaves a relic. What it leaves is fixed by the monster and the tick, so every machine agrees and the RNG is not drawn.
- Villagers pick loot up by day, when no monster is near: coins go into the purse, a trinket or a relic into the pack, to be sold at the stall. Loot nobody takes is gone after three days. It blocks nothing and is not on the map.

### Trading with a neighbour

- A villager who is hungry with coin in their purse and no food in their pack buys a meal from a neighbour within a few paces who has food to spare, instead of walking to the tavern. The seller keeps one meal back when hungry themselves, and will not sell to someone they hold a grudge against.
- The model can `trade`: buy something from someone, or sell them something, at the market's price. The other side agrees when they have the thing (or the coin) and no grudge.

### Paying for a bigger house and the yard

- Each day at half past six, when the village looks over what wants building, one villager with the coin to spare may pay for something of their own: a new room for their small cottage (40 gold, where the village builds one only when beds run short), or something for their yard (a wagon, a strongbox, a barrel, a scarecrow or a hay bale, 6 to 20 gold) on a free pad by their home. The coin goes into the chest; the materials come from the stores as for any project; and the chest pays each builder a wage when the thing is done. The model can `commission` the same when it can afford it.

### Who decides

The routines do all of the above without a model. The prompt header names what a villager carries and how much gold they have, and the market's stalls when they hold anything; the decide prompt names the loot within reach, who nearby has something to sell or buy, and what the villager could pay for, each with its tool only when there is something to choose.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `Character.gold`; `ItemId::Trinket` and `Relic` with their counts on `Inventory`; `SimState.economy: Economy` (the chest and the stalls' stock); `SimState.loot`; `IntentAction::PickUp`, `Trade` and `Commission`; `Project.sponsor`, `price` and `pad_kind`; `ChronicleKind::Trade`. Every new field is left out of a save when zero, empty or at its start, so older saves and the fixtures load unchanged and `SAVE_VERSION` stays 1. Coin is not a `Resource`: neither the arms' cap nor a stockpile's holds it.
- **Numbers** (`server/crates/sim/src/data/economy.rs`): the starting chest and purses, prices, wages, what the stalls keep back, the trader's hour, the loot table, the price of a room and of each yard thing.
- **The system** (`server/crates/sim/src/systems/economy.rs`): paying, selling, buying, the trader, loot, picking up, trading with a neighbour and commissions. `update_economy` runs right after `update_world`. Selling at the stall and shopping are the ends of the `Trade` and `Shop` activities in `intents.rs`; wages are paid where a `Deliver` job completes in `world.rs`; loot is dropped where a monster dies in `combat.rs`; sponsored projects are queued from `queue_projects`.
- **The brain** (`server/crates/brain`): the gold clause and the market line in the header; the `pick_up`, `trade` and `commission` tools and their lines in the decide prompt; their resolution in `validate.rs`. Deliberate changes to the prompt fixture.
- **The wire** (protocol 7, additive): `Economy` in the snapshot and an `economy` event; `Loot` in the snapshot and a `loot` event; `CharacterDetail.gold` and the valuables on `Inventory`; the `trade` chronicle kind.
- **The client**: the chest pill in the top bar, gold and valuables in the inspector, the coin icon in the chronicle, and loot drawn on the ground (`src/render/loot.ts`).

## Measured

Headless, `cargo run --release -p fsw-sim --bin soak -- 60 <seed> --json` on seeds 1 to 3, routines only, in the village the sim invents for itself:

- The chest never runs dry for long: wages and stall purchases drain it by the first week, the trader fills it again as the camp and the mine pass their reserves, and from the second week on the sponsors' payments keep it between 30 and 50 gold at day 60. The trader comes with something to buy on 26 to 44 of the 60 days.
- Nobody is penniless at day 60. The median purse holds 33, 70 and 190 gold in the three seeds; the richest villager, a stall keeper, 691 in one of them. Villagers hold 8 to 16 valuables between them, waiting to be sold.
- Everything that can be paid for is paid for: all four small cottages have their new room and 15 or 16 of the 16 yard pads are built up by day 60, 18 commissions a seed. After that the coin has nowhere to go but the stalls.
- Population and deaths stay in the bands the dynamic-world design measured: 12 or 13 alive at day 60 after 7 to 11 deaths. The twenty-day test in `crates/sim/tests/sim.rs` keeps its bands, and a 60-day run takes 45 to 56 seconds.

## Left out on purpose

- No shops with owners, no theft of coin between villagers, no debts: the chest can run dry and work goes unpaid until the trader comes. (Orc raiders do take coin from the chest, goods off the stalls and materials from the stores; see the raids in `2026-10-04-combat-design.md`.)
- A tavern meal and a bed stay free. Coin buys the market's goods, a room and the yard, nothing a villager needs to live.
- No third pad by a cottage: the ground behind one is not known to be walkable.
- Loot is not on the nav grid and is not a `WorldObject`: it blocks nothing and changes no map.
- Children keep no coin, never trade or commission, and may pick up what they find.
- Once a villager's cottage has its room and their yard is full, their coin has nowhere to go: no second room, no shop of their own, nothing to buy but the stalls' goods. Riches pile up in a few purses by the second month.
