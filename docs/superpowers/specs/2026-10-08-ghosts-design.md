# Ghosts

Some of the villagers the dead cut down do not rest at once. They come back as ghosts: the same person, pale and see-through, seen only after dark. For two or three nights a ghost drifts between the places of its life, troubles the sleep of those it loved, and frightens and chills whoever meets it in the dark. Nobody can fight one. Then it finds rest and is gone.

## What happens

1. **Who comes back.** A grown villager killed by one of the dead comes back about one time in three. A child never does, nor anyone killed by another villager, by fire or by necromancy. The roll is a hash of the world's seed and the villager's id, not a draw from the sim's dice, so a world where nobody comes back runs exactly as it did before ghosts.
2. **It waits.** The ghost waits, unseen, by the dead villager's grave (or where they fell, if there is no grave). The funeral goes ahead as usual.
3. **It rises** at the first dusk (20:30) at least eight hours after the death. The chronicle says "The ghost of … rose from the grave." It will walk for two or three nights, drawn then from the sim's dice.
4. **By night it wanders**, slowly, between its own old home, the homes of up to three of the people it was close to (spouse, partner, kin, or affinity of 25 or more), its grave and the place it fell, lingering 20 to 40 minutes at each.
5. **It haunts houses.** While it lingers at a door, every ten minutes each sleeper inside has a 0.35 chance of a bad night: 15 energy lost and a memory ("Something moaned at the window in the night. I swear it was …"), at importance 8 for someone who loved them and 6 for anyone else.
6. **It is seen.** Anyone awake in the open within 14 units who has not met it in the last two hours reacts:
   - someone who loved them grieves (a grief emote and a memory at importance 8) and does not run;
   - anyone else is frightened unless a roll under their bravery × 0.8 (plus 0.25 on watch) saves them; a coward always runs, a fighter never does. The frightened cry out, run home and remember it at importance 6; the others hold their ground and remember that at importance 5.
   - The first sighting each night goes into the chronicle: "The ghost of … was seen near …".
   There is no alarm and no fight: perception, the monsters and the night's count of the dead do not know ghosts exist.
7. **Its touch.** A ghost drifts after the nearest grown villager awake in the open within 12 units, at a speed a walker can outpace. If it reaches them, it passes through them: 25 energy and 6 to 10 health lost, never below a quarter of their health, so a ghost never kills. They remember it at importance 7 and run home, and the ghost leaves everyone alone for half an hour.
8. **By day** (from 5:36, the dawn that turns the dead to dust) it waits at its grave, unseen. Each dawn takes one of its nights.
9. **It finds rest.** At the dawn of its last night it fades for a few seconds where it stands and is gone. The chronicle says "The ghost of … found rest at last, and was seen no more." Those who loved them remember that they are at peace (importance 7).

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `Ghost` and `GhostPhase` (`waiting`, `abroad`, `resting`, `fading`), in `SimState.ghosts` by id (`g<n>`). The map is left out of a save when empty, so older saves and the fixtures load unchanged.
- **The system** (`server/crates/sim/src/systems/ghosts.rs`): `returns` (the roll), `Haunt::of` and `bind` (taken from the villager before `kill_character`, bound after it), and `update_ghosts`, which runs right after `update_monsters`. Its constants are at the top. The hook is the fatal blow in `combat.rs::damage_character`.
- **Wire** (`server/crates/server/src/api/wire.rs`, protocol version 9): `EntityKind::Ghost`, `GhostDetail` and `Detail::Ghost`. A ghost is encoded with the villager's look, empty hands and `v` true only while it walks or fades (`encode.rs`). `GET /api/ghosts/{id}` serves its card (`digest.rs::ghost_detail_of`), open to guests.
- **Client**:
  - `src/render/characters.ts`: `ActorView` draws a ghost with its own see-through, faintly glowing copies of the shared materials, no shadow, floating a little off the ground, and fading in and out rather than popping.
  - `src/render/scene.ts`: a ghost the server has let go of fades out where it stood; a ring of mist (`fx.ts`) marks its coming and going.
  - `src/render/quality.ts`: `ghostShimmer` (the bob, the flicker and the mist) is off on `low`.
  - A pale label (`overlay.ts`, `styles.css`), the ghost's card in the inspector (`ui/Inspector.tsx`), and the help.

## Tests

- `server/crates/sim/tests/ghosts.rs`: the roll's rate and its dependence on the seed; only monster kills of grown villagers return; a ghost rises at dusk, is unseen by day, walks two or three nights and finds rest, and nobody fights it; the timid run and those who loved them grieve; its touch never takes health below a quarter; a haunting survives a save and load.
- `encode.rs` (a ghost on the wire), `tests/http.rs` (its card, and the route open to guests), `tests/net.test.ts` (the client knows a ghost and follows its visibility).

## Left out on purpose

- Ghosts cannot be fought, banished or blessed away, and the model cannot choose to face one.
- The prompts are not changed; the model learns of a ghost only through what the villagers remember.
- No sound of its own.
- No ghost of a child, of a murder victim, of the fire's dead or of a villager lost to necromancy.
