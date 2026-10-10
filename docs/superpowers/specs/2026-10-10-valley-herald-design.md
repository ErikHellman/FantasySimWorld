# The Valley Herald

October 2026. The chronicle tells everything as it happens, but someone who was away for a day had to scroll back through hundreds of lines to learn what they missed. The Herald is a front page about the day before, ready each morning, and an archive of the last month of them. It is for viewers only: no villager reads it, and nothing in it reaches the village.

## What happens

1. **Dawn.** From six in the morning of the second day, the Herald waits for that day's issue, which tells of the day before.
2. **The model writes it.** The brain asks the model once a day (`RequestKind::Herald`, the same priority as the billboards' words, but allowed while the model has some room rather than only when it is idle). The prompt is the chronicle of the day before, in the order it happened, with the time of each line, leaving out the dawn and the Herald's own announcements; past 40 lines the weightiest are kept. The model answers with `write_herald`: a headline, a lead story, perhaps a second story and a line of gossip. The model is told the readers know everything the chronicle says, secrets included, since viewers are told who the murderer is anyway.
3. **Or the sim sets it.** If no issue is out by eight (no model, a slow one, or an answer with no headline or story), the sim sets the page from the same news: the weightiest line as the headline (a death, then a fire, then a fight or a quarrel, then romance or an arrival, then talk, then trade and the rest; the earliest among equals), the next two as stories, and a bit of romance or talk as gossip. A day with no news gets "All quiet in the valley". This draws nothing from the RNG.
4. **It comes out.** The issue goes into `state.herald` (the last 30 are kept, saved with the world), the chronicle says "The Valley Herald is out: …", and viewers get a `herald` event. An issue is printed once: a late answer from the model is dropped.
5. **Viewers read it.** The 📰 pill (in ☰ on a phone) opens the page, with ‹ Earlier and Later › through the archive (`GET /api/herald`). The browser remembers the last issue it opened (`fsw.herald` in local storage). On the first snapshot of a page load, a viewer who has been here before and missed an issue finds the Herald open, headed "While you were away". A first visit only remembers where the viewer starts; an issue that comes out while someone watches marks the pill New rather than opening; a new world (an issue older than the one remembered) starts the count again.

## Where it lives

- `server/crates/sim/src/systems/herald.rs`: when an issue is wanted, the news it may tell of, the sim's own page, tidying and publishing. `HeraldIssue` and `SimState.herald` in `state.rs`, `Command::WriteHerald`, `SimEvent::Herald`.
- `server/crates/brain/src/brain.rs` (`write_herald`), `prompts.rs` (`herald_prompt`), `tools.rs` (`write_herald`), `validate.rs` (`interpret_herald`).
- `server/crates/server/src/api/wire.rs`: `HeraldIssue`, `Snapshot.herald` (the latest), `WireEvent::Herald`, `HeraldArchive` for `GET /api/herald` (open to guests). No protocol bump: older clients ignore the new event and field.
- `src/ui/Herald.tsx` (the page and the pill), `setHerald` in `src/ui/store.ts`.
- `tools/mock-llm.mjs` answers `write_herald` (and now `write_billboard`) for trying the model path.
- Tests: `server/crates/sim/tests/herald.rs`, a brain test in `server/crates/brain/tests/brain.rs`, an encoder test, the route in `server/crates/server/tests/http.rs`, and `tests/herald.test.ts` for when the page opens by itself.

## Left out on purpose

- **No villager reads it.** A paper the village read would need a writer among them and would let the model's words steer the sim; the billboards already do that.
- **No database table.** Thirty issues are small enough to keep in the saved world, which also carries them through export and import.
- **No catching up on missed days.** If the server was down through a morning, that day has no issue; the next one tells only of its own yesterday.
- **No notifications.** Telling a viewer who is not on the page belongs with the favourites and notifications of a later stage.
