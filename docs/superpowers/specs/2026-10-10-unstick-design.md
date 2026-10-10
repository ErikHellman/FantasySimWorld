# Nothing stays stuck

October 2026. Someone walking on the spot, legs going and getting nowhere, is the quickest way to spoil the illusion of a living village. Each system already handles the snags it knows of: a villager in someone's way asks them to move (`systems/blocking.rs`), a fighter who cannot reach an opponent gives up (`systems/combat.rs`), and an animal that cuts a corner finds its way again (`systems/animals.rs`). This stage adds a net under all of them for the snags nobody has thought of yet.

## What happens

- **Who is watched.** Once a game minute, `systems/stalls.rs` looks at everyone with a path to walk: villagers out of doors on their way somewhere (not fighting, which has its own way of giving up), animals that are not ridden or flying, the dead and raiders on foot, and ghosts. For each it keeps where they last got somewhere and when (`SimState.stalls`).
- **Getting nowhere.** Moving half a metre counts as getting somewhere. Anyone who has not done so for eight game minutes (eight seconds at normal speed) is helped on:
  1. Within a metre and a half of where they were going, they are there: their path is dropped, which every system takes as an arrival.
  2. Otherwise they are set back on open ground if they stand on blocked ground, and find their way afresh. If the fresh way is the one they had, they step round whatever holds them: two metres to one side (which side is settled by a hash of their id), and then on.
  3. Helped three times without getting anywhere in between, they give up on going there; their own system picks what to do next.
- **The test.** `crates/sim/tests/stuck.rs` watches two villages for six days, once a game minute, and fails if anything with a path walks in place for 30 game minutes, naming who, where and what they were doing. On `main` before this stage it caught villagers walking to a spot right beside someone standing still: the push that keeps bodies apart undid each step, and the one standing was beside the way rather than in it, so nobody asked them to move. Two smaller tests check the guard on its own.

## Left out on purpose

- **No new snags fixed at the source.** The guard catches them; the test names them, so a cause that turns up often can still be fixed where it starts.
- **No RNG.** A world in which nothing is stuck runs exactly as before. Worlds in which something was stuck now run differently from that point on, which is why the prompt and frame fixtures were regenerated (no prompt wording changed).
- **The dragon** flies and is never stuck; **conversations, sleep and work in place** have no path and are not watched.
