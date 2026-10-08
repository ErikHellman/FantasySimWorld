# A world that changes: felling, mining, taking apart, burning, and building again

The map used to be fixed: trees, rocks, props and buildings were generated once, in two matching copies, and only a home on fire ever changed. Now villagers change the world with their hands, and the village grows it back. This is also the start of an economy: wood, stone and ore are gathered, kept, carried and spent.

Decisions made with the owner: felled trees and built things change where one can walk; the economy runs on materials and stockpiles (coin and trade come later, on top); anyone may do anything, including burning or taking apart a neighbour's home, and the world pushes back through consequences and rebuilding rather than rules; everything is drawn from the committed models plus a little procedural geometry, because the KayKit collection was not to hand.

## What happens

### Gathering

- **Woodcutters fell real trees.** A work block of chopping wood goes to the nearest standing tree within 40 paces of the lumber camp instead of the slot animation (the slot is used when none is left). Felling takes 90 villager-minutes, shared by up to two; the tree sways under the axe, leans over for a second and a half, and leaves a stump. It yields four logs (five from a big one), which the woodcutter carries to the lumber camp. Work on a tree nobody has touched for half an hour lapses.
- **Miners break real rocks** within 45 paces of the mine: 120 villager-minutes for three blocks of stone (one from a small rock) and a lump of ore in every third. A mined rock is gone for good.
- **Farmers tend the meadows** every fourth working day: a scythe cuts a hex's grass (45 minutes, grows back in three days) or a spade digs a pit (60 minutes, a block of stone in every fourth, filled in after four days).
- **Materials** are counted, not carried as items: a villager holds six units at most and puts them down at the stockpile that takes them (wood at the lumber camp, stone and ore at the mine, 60 each; the market takes the overflow, 120). The slot work at the camp and the mine now yields a log every 40 minutes and a block every 50.

### Taking apart and burning

- **Props come apart** (40 villager-minutes) into pieces that lie there ten days and then are gone; a log or a block of stone is kept, by what the thing was made of. **Buildings come down to their footings** (600 villager-minutes, five logs and two blocks back); the rubble is walked round, whoever was inside is put out, the chronicle names who did it, and everyone remembers. A home taken down is no home: its people sleep rough by the campfire and newcomers are not given it.
- **Anything can be set alight** with a torch held to it for eight seconds. A burning building musters the village to the buckets as a home fire does and is a ruin if it is lost; up to two buildings may burn at once. A burning prop is not fought: it burns to ash in half an hour, and the ash blows away in six days. Three things may be alight in all.
- **Whoever lit it is held to blame**: the chronicle names them, those who saw it and those whose home it was take a heavy grievance against them (45 and 60 on the hostility ladder) and are asked what they make of it, and the arsonist is not called to carry water to their own fire. Routines light fires only for the spiteful at the lethal rung of a feud, by night, and rarely; the model may choose to burn anything.
- **Left alone**: the graveyard, the ruins, the signpost at the road's end, lamps, the well, the campfire, flowers and the crops. Mountains, hills and the dense woods are the border and have no ids.

### Growing back and building

- **Trees grow back** on a fixed clock: stump for two days, sapling for five, young tree for six, then the map's own tree again, and the ground it shades is blocked again.
- **The village builds.** Once a day at half past six it looks over what wants doing and takes on at most two projects, in this order: raise a building that was taken down or burnt to a ruin (12 logs and 6 blocks, 1,500 villager-minutes); put back a prop that is gone (2 logs or 2 blocks, 60 minutes); make the fullest small cottage bigger when beds run short (20 logs and 10 blocks, 2,400 minutes; it has a bed more afterwards and newcomers are sent there); and, when the lumber camp holds thirty logs, put up something new on one of 24 pads authored inside the places' clearings: a fence, a stack of logs, a bench, a crate of goods or a bed of flowers (2 to 6 logs, 180 minutes), chosen by the project's id so every machine agrees. A home burnt by the dead is still rebuilt the old way, for work alone.
- **Every other working day** a villager's hours go to the project: they fetch what it lacks from the stores, carry it to the site, and put hands to it once the materials are there. The model can choose `build` as well. A building being raised shows scaffolding and the timber stacks beside it.

### Who decides

The routines do the ordinary work (felling, mining, tending, carrying, building) and never tear anything down. The model gets a `work_on` tool whenever something is within reach, with the prompt naming the nearest trees, rocks, the ground underfoot, buildings and props ("Things you could work on nearby (work_on): a tree (9 within reach) (fell); a rock (mine); the ground here (mow); the blue house (dismantle or burn); …"), and its loose names are resolved against that list. Carrying is never chosen; it follows from gathering.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `SimState.objects` holds only what differs from the map, by id: `s:<scatter index>` for a tree or rock, `p:<place>:<model index>` for a prop, `b:<place>` for a building, `h:<col>,<row>` for a hex's ground, `n:<n>` for something built. `WorldObject` carries a stage, the tick it began, the work put into it, its fire and, for built things, its placement. `Character.carried` and `SimState.stockpiles` are `Store`s (a count per `Resource`); `SimState.projects` is the building queue; `Director.next_project_tick` the daily look. All of it is left out of a save when empty, so older saves and the fixtures load unchanged and `SAVE_VERSION` stays 1. `houses` is untouched: a home's fire and ruin still live there.
- **Walking** (`server/crates/sim/src/nav.rs`): every path and walkability check in the sim goes through `NavOverlay`, a cache derived from `objects` that holds no copy at all while nothing has changed. A felled tree frees the cells its trunk blocked (unless something else still covers them); a stump, a pit, rubble and a built thing block theirs. `fsw_world` gained `*_on` variants of its path functions that take any cost slice, `terrain_cost` and `block` as the pieces the static grid is built from, and `obstacle_of` on scatter and places so a model's obstacle can be found. The static grid, `worldHash` and the golden fixtures are as they were.
- **The system** (`server/crates/sim/src/systems/world.rs`): `WorldWork { job, target, minutes }` is the one new intent, with `WorldJob { Fell, Mine, Dig, Mow, Dismantle, Burn, Build, Deliver }` and a table in `data/jobs.rs` (swing, tool, minutes, how many may share). `run_world_work` walks to a spot round the target fixed per person, works a minute at a time with the work kept on the object so helpers share it, and `complete_job` changes the world. `update_world` runs right after `update_fire` and keeps the clock (falling, stumps, saplings, pits, grass, pieces, ash, lapsed work) and queues the day's projects. Every change to `objects` goes through `put_object` and `drop_object`, which refresh the overlay. Building pads are `PadDef`s in `place_defs.rs`, Rust only: what stands on one reaches the client with its own placement.
- **Fire** (`server/crates/sim/src/systems/fire.rs`): a fire's site is a home's place id or an object id; `ignite_site` lights either, `burning_sites` and `alarm_sites` list them, and the muster sends each villager to the nearest burning building, skipping whoever lit it (`Character.last_arson_tick`).
- **The brain** (`server/crates/brain`): the `work_on` tool, the targets line (priority 2, at most six entries within 30 paces plus buildable projects), `resolve_target`, and `chop_wood` and `mine` from `do_activity` turned into real trees and rocks. The header names what a villager carries and what the stores hold when there is any. The prompt fixture was regenerated for these lines and for the worlds they give.
- **The wire** (protocol 6): `WorldObject` (id, kind, name, stage, the tick it began, progress and fire in steps of five, placement for built things), `Stockpile`, `Goods` on the inspector's detail, `Snapshot.worldObjects` and `Snapshot.stockpiles`, and the `world_object` and `stockpile` events, found by comparing state as houses are; a stage of `intact` means the thing is as the map has it and can be forgotten.
- **The client**: `src/render/vegetation.ts` draws the trees and rocks as instances (one pool per model) so one can sway, fall, stand as a stump, grow back or vanish by changing a matrix, timed from the sim's tick; `src/render/structures.ts` (from `houses.ts`) keeps every building as its own object with a ruin, a bare plot and rubble, scaffolding, the bigger cottage and up to two fires; `src/render/marks.ts` shows pieces, ash, a small flame, pits, mown ground and what the village built, and hides props and grass in the merged scenery by the handles `SceneryBuilder` now gives out, baking a changed chunk again one per tick. The top bar shows the stores, the inspector what is carried, the banner what is alight.

## Measured

Headless, `cargo run --release -p fsw-sim --bin soak -- 60 <seed>` on seeds 1 to 3, routines only, in the village the sim invents for itself (the soak founds one without a model):

- Felling and mining start on the first morning. The lumber camp holds its 60 logs within a week in every seed, and the market's overflow holds 10 to 120 logs at day 60; stone comes in slower with fewer miners, so the mine holds 45 to 59 blocks and a lump of ore and the market none.
- The village takes on one project a day and finishes it the same morning: all 24 pads are built up by day 25 or 26 in two seeds, and by day 48 in the third, where timber ran short. Nothing is raised or enlarged, because routines never take a building down and beds are not short.
- No fire at all in 60 days on these seeds, by villagers or the dead: the watch holds and the gentler dead leave more people alive to stand it.
- The twenty-day test in `crates/sim/tests/world.rs` (and its sixty-day `--ignored` twin) holds four of five trees standing and at least six of eight homes whole every day; the three seeds end with 29 to 47 changed things in the world and a population of 11 to 13.

## Left out on purpose

- No new places or homes: a `Place` is static on both sides, so a bigger village means a layout change in both twins. Small cottages grow instead.
- Trees do not burn and fire does not spread; the field is there for later.
- Coin and trade came later, on top of the materials: see [the economy design](2026-10-08-economy-design.md). The stores no longer saturate, because a passing trader buys the market's surplus.
- No new sounds (a chop, a fall, a crash) and no new models: saplings are scaled trees, ash is dark discs, scaffolding is boxes.
- The routines never dismantle or (but for the spiteful at the lethal rung) burn; only the model does, so a village without one keeps its buildings.
