# Block, dodge, orc raids and bosses

**Status (2026-10-08): built**, all three steps, plus an admin's "Call a raid". Each step's "Built" section says where the code differs from the plan and what the soak measured.

Fights today are an exchange of blows. Every swing that is still in reach when its windup ends lands, and the only enemies are the dead, who rise at night and crumble at dawn. This design makes combat richer in three steps, each shipped on its own:

1. **Block and dodge.** Defenders can turn a blow aside or step out of it, so skill and equipment show and fights last a little longer.
2. **Orc raids.** A rare threat by day: a war band comes up the road to plunder, and leaves when it has its loot or has had enough.
3. **Bosses.** A Skeleton Golem rises on a grim night, and an Orc Warchief leads some raids. The whole village has to rally to bring either down.

Every new rule runs in the sim, draws from the sim's RNG in a fixed order and is saved with the world, so the world stays deterministic and replayable.

## Step 1: new models and clips, then block and dodge

### Assets

`tools/assets/asset-list.mjs` takes these from the KayKit collection (CC0):
- **OrcRaider**: medium rig, with Orc_Axe and Orc_Club.
- **OrcBrute**: large rig, with Orc_Axe_Large.
- **Skeleton_Golem**: large rig. Drawn larger than the Barbarian_Large villagers.
- **Clips for both rigs**: `Dodge_Forward`, `Dodge_Backward`, `Dodge_Left`, `Dodge_Right` and `Melee_Block_Hit`.

`pnpm assets` regenerates `src/generated/assetManifest.ts`, `src/generated/tileHeights.ts` and `server/data`. The new anim ids are added to both `src/render/anims.ts` and `server/crates/sim/src/data/anims.rs`:
- `block_hit`
- `dodge_back`, `dodge_left`, `dodge_right`

### What happens

1. **A blow lands on a defender.** The defender must be in a fight (a villager with a combat target, or a monster that is chasing) and must not be in the middle of its own swing (`hit_tick` is in the past). Anyone else, such as a sleeper or someone walking past, takes the blow as today.
2. **Block first.** A defender whose weapon has a block chance rolls for it. A block cuts the damage to a quarter, plays `block_hit`, and rings out with a clang.
3. **Then dodge.** If the block fails, or the weapon cannot block, a defender with a dodge chance rolls for that. A dodge takes no damage and plays a `dodge_*` clip. The defender steps 1.2 units away from the attacker, to the side when straight back is not walkable.
4. **The chance:**
   - base: the weapon's chance, from the class or monster kind
   - plus the defender's bravery × 0.1 (villagers only)
   - plus 0.08 for the Fighter quirk
   - minus 0.1 below 30% health
   - never above 0.5

   After a block or a dodge the defender cannot defend again for 0.6 seconds, so a flurry still gets through.
5. **Ranged blows and slams.** Arrows, bolts and spells cannot be blocked; they can be dodged at half the chance. A boss's slam (step 3) cannot be blocked either.

Starting chances:

| Who | Block | Dodge |
| --- | --- | --- |
| Knight | 0.35 | — |
| Paladin | 0.30 | — |
| Swordsman | 0.25 | — |
| Barbarian | 0.20 | — |
| Rogue | — | 0.25 |
| Scout | — | 0.25 |
| Ranger | — | 0.20 |
| Skeleton Warrior | 0.30 | — |
| Skeleton Rogue | — | 0.20 |
| Orc (axe) | 0.15 | — |
| Orc (club) | — | 0.10 |

Classes not listed have neither, and nor does a bare fist.

### Where it lives

- **Data**: `block` and `dodge` chances on `Weapon` (`server/crates/sim/src/data/classes.rs`) and on `MonsterDef` (`data/monsters.rs`).
- **State**: a `defend_ready_tick` on `CombatState` and on `Monster`, left out of a save when zero, so older saves and the fixtures load unchanged.
- **The rolls**: in `combat.rs::land_hit` and in the monsters' hit path (`monsters.rs`), through one shared `defend()` helper. The rolls use the sim's RNG, one draw per defence that has a chance above zero.
- **Wire**: `WireEvent::Hit` gains `outcome`, one of `hit`, `blocked` or `dodged` (default `hit`). Older clients ignore the field, so `PROTOCOL_VERSION` stays as it is. `export-api` and `export-frames` are re-run.
- **Client**:
  - `src/render/fx.ts`: sparks for a block, a dust puff for a dodge
  - `src/audio/ambient.ts`: a clang and a whoosh, from CC0 sources added to `tools/audio/sources.ts` and credited in `public/audio/CREDITS.md`

### Balance

Before and after, run `cargo run --release -p fsw-sim --bin soak -- 30 <seed> --json` for seeds 1 to 10. Deaths per village and monster kills should stay within 15% of today's. If villagers now survive too easily, monster damage goes up a step. `sim/tests/sim.rs::long_run` keeps its bands. The measured numbers go into this spec when the step is done.

## Step 2: orc raids

### What happens

1. **A raid is due.** From day 4, a raid comes every 5 to 9 days, between 9:00 and 16:00. It comes only when at least 8 villagers are alive and no house is burning. Otherwise it waits for the next day.
2. **The band comes up the west road.** The band has 3 + (alive ÷ 6) orcs, between 3 and 6, mixing axes and clubs. The west road is the only way into the valley, so the map does not change. Orcs and the dead share the cap of 14 monsters out at once.
3. **The village is warned.** About twenty seconds after the band appears, everyone gets an alarm: "Orcs on the road!". A war horn sounds and the music turns tense. From then on villagers react as they do to the dead: the brave and the watch stand their ground, the timid run home or to the tavern, and the model is asked through the usual `react` request.
4. **Orcs plunder.** Each orc walks to a place worth robbing: the market, the farm, the smithy, or a home with nobody in it. It works there for five seconds (`Interact`), adds one to the band's loot, and the place is plundered for one game day. While a place is plundered, the work done there gives no food, potions or gifts.
5. **Orcs fight when crossed.** An orc fights whoever attacks it or comes within 3 units. It gives up a chase that takes it more than 12 units from where it was going, and an orc left on its own below a quarter of its health runs for the road.
6. **The band leaves.** The band goes back down the west road and disappears at the edge when any of these happens:
   - its loot reaches its size
   - half of it has fallen
   - its total health drops below 40%
   - it is 17:00
7. **The village remembers.** The chronicle says either "Orcs raided the village and carried off …" (naming the plundered places) or "The village drove the raiders off". Those who fought or saw the raid remember it. The server saves.

Orcs are not the dead. They do not rise from graves, they do not crumble at dawn, and their bodies are not buried; a fallen orc's corpse lies for a while and then is gone.

**Plunder, as built.** The owner chose both halves: a plundered place is spoiled for a game day, and raiders carry off real goods. At the market an orc takes up to 12 coins from the chest and up to 3 goods off the stalls; at any stockpile (the market, the lumber camp, the mine) up to 10 materials. A farm, the smithy or a home is only spoiled. A raider who falls drops what it carried where it fell, as loot the villagers pick up (coins into the purse, goods into the pack, materials into their arms and on to the stores); what the band takes down the road is gone. This reverses the economy note's "no theft of coin".

### Where it lives

- **Kinds** (`data/monsters.rs`): `MonsterKind::OrcAxe`, `OrcClub` and `OrcWarchief` (step 3), and a `faction()` returning `Undead` or `Orc`. Daylight, rising, burial and arson apply to the undead only. Orc corpses are not buried.
- **The system** (`server/crates/sim/src/systems/raids.rs`, new), run from `director.rs`: scheduling, the band's spawn, the warning, choosing targets, plundering and the retreat. Its constants are at the top of the file.
- **State**:
  - `Director.next_raid_tick` and `Director.raids`
  - `Monster.raid: Option<RaidOrder>`, the orc's target and whether it is leaving
  - `SimState.raid: Option<Band>`, the band's loot, size and plundered places
  - `SimState.plundered: BTreeMap<PlaceId, i64>`, the tick each place recovers

  All of these are `serde(default)` and left out of a save when empty.
- **Plunder's effect**: `intents.rs::give` checks the place.
- **The brain** (`prompts.rs`): the world text "the only monsters are the walking dead" becomes "the dead rise at night; orc raiders sometimes come up the road by day". Prompts say "raiders" for orcs in sight. This is a deliberate change to `prompts.json`.
- **Wire**: a new `WireEvent::Raid { phase }` (`coming`, `leaving` or `gone`) for the horn and the music. `MonsterSpawn` and `look` already carry the rest. Older clients ignore an event they do not know. `export-api` is re-run.
- **Client**: the war horn (`src/audio`, CC0, credited), the `tension` mood during a raid (`src/audio/mood.ts`), and the chronicle kind `combat`.

### Built

Built as above, with these details settled in the code:
- **When**: the first raid is fixed by the seed (day 4 to 6, 9:00 to 15:00), so nothing is drawn and nothing written until it comes; `crates/sim/tests/raids.rs` holds a world before its first raid equal to one without raids. After that the next is drawn when a band appears. A raid that is due but cannot come waits for the same hour the next day.
- **The band**: `MonsterKind::OrcAxe` and `OrcClub` (the OrcRaider model, two hides, some without a pack), at the end of `MONSTERS` with weight 0. They are as tough as a knight (150 and 130 health, 13–19 and 11–16 damage); at the first trial's 70 and 60 the village cut every band down within minutes and nobody was ever hurt.
- **Where it lives**: `systems/raids.rs` is run from `update_director` (the schedule, the warning, the band's end) and from the monsters' loop for each orc (`update_orc`), so the system order in `Sim::step` is unchanged. The warning is skipped if someone has already seen the band and shouted. The alarm, the stand-down, panic and the prompts name raiders rather than the dead when it is orcs (`Director.alarm_raid`).
- **Spoiling**: `activity_minute` skips an activity's goods at a plundered place (the check needs the slot's place, which `give` does not have).
- **Bodies**: an orc dies like one of the dead (five seconds of `Death`), then lies as a corpse in the death pose for two hours; no grave.
- **Wire** (protocol 9: the new loot kinds would break an older page): `WireEvent::Raid`, `LootKind::Goods` and `Materials` with an `item`, `MonsterDetail.faction` and `carrying`, `Status.raid` (the banner and the music). Orcs send no `monster_spawn`: they walk up, they do not claw out of the ground.
- **Sound**: the war horn is StumpyStrust's "Their Coming" (CC0); a fallen orc plays the villagers' fall, not the skeleton's clatter.
- **Calling a raid** (added at the owner's request): `POST /api/admin/raid`, behind `require_admin`, sends `Command::CallRaid`, and **🛠 Admin → World → Call a raid** sends that. A called raid skips the schedule and the bar of eight villagers (three will do) but still waits for daylight, no fire and room on the road; it is refused (409) while the village is being settled, while a band is in the valley or when one is already called. Drawing nothing itself, the command keeps a replay of the command stream exact.

Measured over 30 days, seeds 1 to 10 (means per village): 4.2 raids; 9.4 places plundered; 0.9 villagers killed by orcs; the band got away with plunder 0.8 times. Villagers dead, all causes, 6.4 (6.5 before raids). Most bands turn for home on losing 60% of their health, 20 to 50 minutes after they appear; a few take all they came for. The chest ends the month much as it did without raids. In practice the raiders seldom get past the farm, the lumber camp and the nearest cottages before the village is on them.

## Step 3: bosses

### The Skeleton Golem

- **What it is**: one of the dead, on the large rig, drawn larger than anyone else.
  - health 600, speed 1.9
  - its slam does 25–40 damage, reaches 2.6, and comes every 2.8 seconds after a 1.2-second windup
- **The slam** hits every villager within 2.8 of where it lands, not only its target. It cannot be blocked. The long windup gives warning, so dodging it is 1.5 times as likely.
- **When it rises**: on a grim night, in place of the necromancer. The first comes from night 10, then one every 8 to 12 nights, and only with at least 10 alive.
- **It does not crumble at dawn.** It lies low at the ruins by day, as `lasting` monsters already do, and comes back at night until it is destroyed.

### The Orc Warchief

- **What it is**: an orc on the large rig (the OrcBrute model).
  - health 420, damage 20–32, a swing every 2 seconds
- **Its cleave** hits up to two villagers in front of it.
- **When it comes**: it leads every third raid from day 12, with at least 10 alive.
- **When it falls**, the band breaks and leaves at once, carrying whatever loot it has.

### For both

- **The rally**: when a boss appears, everyone hears the alarm, and anyone with bravery of 0.45 or more answers it (the usual bar is 0.6).
- **Bringing one down**: the chronicle names everyone who fought it, each of them remembers it at importance 9, and the server saves.
- **The client** shows a health bar for a boss in sight at the top of the screen.

### Where it lives

- `MonsterKind::SkeletonGolem` and `OrcWarchief` (`data/monsters.rs`), with `boss: true` and the area hits in `monsters.rs`.
- The grim night in `director.rs`; the warchief's raids in `raids.rs`.
- The rally in `perception.rs`.
- **Wire**: `EntityStatic` gains an optional `boss` flag. The model's draw scale comes from `asset-list.mjs`.
- **Client**: the boss bar in `src/ui`.

### Built

Built as above, with these differences:
- **The golem's numbers**: at the planned 25–40 damage every 2.8 seconds its slam killed four villagers a month in each village, as fighters crowd round it. It is built at 18–28 every 3.2 seconds, after a 1.2-second windup, with 600 health and a reach of 2.6; it then kills about one villager a month and is always brought down. It drops a relic like the necromancer, and it never sets fires.
- **The warchief's cleave** takes the one it swings at and one more in front of it; he is built with block 0.15 like the axe raiders. He leads every third raid (the third, sixth, …) from day 12 with at least ten alive, spawns first so the draws come in a fixed order, belongs to the band without counting in its size or its fallen, and when he falls the band turns for home at once.
- **The large rig** borrows the `Skeletons_*` clips, so the golem rises, walks and crumbles like the rest of the dead. The models are drawn at 0.9 (golem, 3.8 units tall) and 0.8 (warchief, 3.4); the asset build allows any scale on the large rig for a model that never sits (`seats: false`).
- **The rally**: a golem's rising raises the alarm and stirs everyone (`director::rally`); a warchief comes with his band's warning. While any boss is abroad, perception's responder bar is 0.45.
- **Remembered**: every villager who struck the boss is kept in `Monster.foes`; when it falls the chronicle names them and who struck the last blow, each remembers it at importance 9 with the others as subjects, and the server saves (`SavePoint { "boss fallen" }`).
- **The client**: `EntityStatic.boss` (optional) and `MonsterDetail.boss`; `src/ui/BossBar.tsx` shows the nearest boss within 60 units of the camera.

Measured over 30 days, seeds 1 to 10 (means per village): 1.9 golems rose and all were brought down; 1.0 villagers killed by golems, 0.3 by a warchief and 0.3 by orc raiders. Villagers dead, all causes, 6.8 (6.6 before any of this work); monsters destroyed 177 (168).

## Tests

- **`server/crates/sim/tests/combat.rs`** (new):
  - over many seeded blows, block and dodge rates match their chances
  - nobody blocks or dodges in mid-swing, asleep or out of a fight
  - ranged blows are only ever dodged
  - the cooldown holds
- **`server/crates/sim/tests/raids.rs`** (new):
  - a raid comes on schedule by the west road, and not when a house burns or the village is small
  - plundering marks a place, and it gives nothing while marked
  - the band leaves when it has its loot, has lost half its number, or it is 17:00
  - the warchief's fall breaks the band
  - a save in the middle of a raid round-trips
- **`server/crates/sim/tests/bosses.rs`** (new):
  - a golem rises on a grim night and lies low by day
  - its slam hits everyone near the point of impact and is never blocked
  - the rally answers at bravery 0.45
- **`sim/tests/sim.rs`**: its bands and the 200-day soak (`--ignored`) stay green.
- **Fixtures**: `tests/fixtures/server-frames.json` and `src/generated/api.ts` are regenerated, and `tests/net.test.ts` still passes.

## Left out on purpose

- No counter-attack after a block (`Melee_Block_Attack` stays unimported).
- Villagers do not choose to block or dodge. The model is not asked; it is a matter of skill and luck.
- No village store of goods. Plunder spoils places for a day instead of emptying a larder.
- No orc camp, no parley and no trade.
- No orc raid in the viewer shop yet.
- No other large-rig monsters (FrostGolem, Monstrosity, BlackKnight). They can follow the golem's pattern later.
