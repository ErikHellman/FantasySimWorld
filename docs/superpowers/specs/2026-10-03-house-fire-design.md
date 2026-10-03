# House fires

A catastrophe the whole village has to answer. When not one villager is awake, the dead set a home on fire. Everyone is woken to carry water. If they fail, the house burns down, the people who lived there sleep rough, and the village builds it again by hand.

## What happens

1. **The dead choose a house.** A roaming monster, at night, checks whether every living villager is asleep. If so (and nothing is burning, and the last fire was at least six days ago) it walks to the nearest home that stands and has someone living in it. The plan is dropped the moment anyone wakes, or the monster starts chasing someone. At the door it works for four seconds, and the house catches.
2. **The house catches.** Whoever is inside is thrown out of bed onto the street and burnt (6 to 14 hit points; this can kill the already wounded). The first of them shouts "Fire!". The chronicle records it and the server saves.
3. **Everyone carries water.** Every villager who is not fighting, fleeing, beaten or badly hurt (under a quarter of their health) is woken and joins the bucket chain: run to the nearest water (the well on the green or the lake shore), fill a bucket, run back, throw it. Each bucket knocks the fire back; now and then it costs a scorched arm. The model cannot take anyone off the chain: only a fight or flight comes before a burning house.
4. **The dead are still there.** With a house on fire people find courage (they stand their ground more readily), the timid keep carrying water until the dead are right upon them, and those who do run hide for three minutes rather than twelve.
5. **Saved or lost.** The fire starts at 20 out of 100 and grows every game minute, faster the bigger it is, so a fire that is not being beaten back soon gets out of hand. At 0 it is out: the helpers are named in the chronicle, remember it, and trust each other a little more. At 100 the house is a ruin.
6. **Homeless.** The people of a ruined house keep it as their home, but cannot use it. They sleep on the ground by the campfire in the glade: a poorer night's rest, out in the open where the dead can find them. When danger comes they hide in the tavern. Newcomers are not given a ruin.
7. **Rebuilding.** A ruin needs 6000 villager-minutes of work. While one stands, working hours go to it: every day for the people who lived there, every other day for everyone else, so the fields are still tended. The model can also choose `rebuild`. When the work is done the house stands again and its people sleep indoors.

Measured with the soak (`cargo run --release -p fsw-sim --bin soak -- 60 <seed> --log`, seeds 1 to 10): no fire in the first four weeks, because the watch keeps someone awake every night; after that, between none and two in sixty days, in villages that have lost their guards. A full village of thirteen saves any house in 15 to 40 game minutes. Nine save most, six lose most, four lose all. A ruin is rebuilt in four to six days.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `SimState.houses` lists only the homes that are burning or in ruins (`House { phase, fire, built, sinceTick, helpers }`); `Monster.arson` is a monster's plan; `Director.lastFireTick` holds the cooldown. All three are left out of a save when empty, so older saves load and the fixtures are unchanged.
- **The system** (`server/crates/sim/src/systems/fire.rs`): arson, ignition, the muster, the bucket chain (`IntentAction::FightFire`), putting out, burning down, rebuilding work and the helpers for the homeless. Its constants are at the top of the file. `update_fire` runs right after `update_monsters`. Nothing in it draws from the RNG or changes anything until a fire is possible.
- **Rebuilding** is `ActivityId::Rebuild`: it has no slot (people stand around the ruin's door), and it is not among the activities always on offer.
- **The brain** (`server/crates/brain`): the prompt header says when a house is burning or in ruins, and whether it is the reader's own; `rebuild` is listed and accepted only while a ruin exists. Villagers on the bucket chain are not asked to decide anything.
- **The wire** (`wire.rs`, protocol version 3): `Snapshot.houses` and the `house` event (`intact`, `burning` or `ruin`, the fire in steps of five, the rebuilding in percent), found by comparing state as billboards are; the chronicle kind `fire`; `CharacterDetail.homeless`.
- **The client**: `src/render/houses.ts` keeps each home as its own object (the rest of the scenery is merged), draws flames and smoke over the burning one, swaps a ruined one for `hex/building_destroyed` and stacks timber beside it as the work goes on. The fire borrows one light from the lamp pool. The banner, the chronicle's 🔥 and the inspector's home line are in `src/ui`. The glade's campfire loop doubles as the sound of the blaze, and the alarm bell rings at the cry of "Fire!".

The nav grid does not change: a ruin keeps the footprint of the house, so `worldHash` and the golden fixtures are as they were.

## Left out on purpose

- No wood or stone: rebuilding costs work, not materials.
- One fire at a time, and fire does not spread to the next house.
- A plan for the day cannot hold `rebuild`; the routine turns working hours over to it instead.
- No new sounds: no splash for a bucket and no crash when the house falls.
