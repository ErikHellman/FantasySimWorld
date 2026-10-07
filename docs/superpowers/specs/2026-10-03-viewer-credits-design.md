# Viewer credits: earn by watching, spend on world events

> **Removed on 2026-10-07.** The village is watched, not steered: credits, the shop, bought monsters, blessings and prayer payouts are gone from the server, the sim and the client. This document stays as the record of what was built and why.

## Context

Viewers only watch the village today; nothing they do reaches the sim. This adds a small economy: a viewer who is actively watching (tab visible and focused, reporting every 30 s) earns credits, and spends them on events in the world, priced by how much they change it.

Decisions already made with the owner:

- **Events in the first version:** gifts for a character, a monster at the camera's focus, large threats (raid, necromancer), blessings (heal, feast).
- **Who earns:** signed-in accounts keep a balance in SQLite. In `AUTH_MODE=off` and token mode (no accounts) the balance is per tab and in memory, keyed like the chat (`ChatKey`).
- **Attribution:** the buyer's chat name appears in the viewer chat **and** in the chronicle (so it enters saves and exports).
- **The villagers know who did it.** To them a viewer is a named unseen power. They thank, plead with and pray to that viewer by name ("Spare us, Erik!", "Thank you, lord!"). See "Powers and prayer".
- **Bought monsters** ignore daylight and crumble after a set time.

Facts that shape the design: the sim has no gold, chests or ground items (inventory is food/potion/gift, cap 3 each); the outside reaches the sim only through `Command`s; no non-admin route changes the world yet; the camera target is already exposed in `main.ts`.

## Design

### Earning (server is the only judge)

- Client posts `POST /api/presence {viewer}` every 30 s, only while `document.visibilityState === 'visible' && document.hasFocus()`.
- Server (`credits.rs`, new `CreditsHub` on `AppState`) keeps per `ChatKey` in memory: last counted beat and leftover seconds.
  - The tab must have an open stream (`World.connected` holds the tab id), else 409.
  - Gap since last counted beat under 25 s: ignored (spam and extra tabs of one account earn nothing more). Over 45 s: starts a new run, earns nothing. Otherwise: counts 30 s of watching.
  - Award `floor(seconds × CREDITS_PER_MINUTE / 60)`, keep the remainder, stop at `CREDITS_CAP`.
- Settings in `config.rs`: `CREDITS_PER_MINUTE` (default 2; 0 turns the whole feature off) and `CREDITS_CAP` (default 400). New accounts start with 10.
- A script can still post beats without focus; the server-side pacing bounds that to the honest rate, which is the most a browser-reported signal can guarantee.

### Storage

Migration 3 in `store/migrations.rs` (new file `store/credits.rs`):

- `credits(user_id PK → users ON DELETE CASCADE, balance, earned_total, spent_total, power_name NULL, updated_at)`.
- `credit_purchases(id, user_id NULL, name, world_id, event, cost, tick, at)`: an audit trail, pruned after 90 days in `maintenance.rs`.
- `Store::earn(user_id, amount, cap) -> balance` and `Store::spend(user_id, cost) -> Result<balance, Short>` as single atomic statements (`UPDATE … WHERE balance >= ?`), plus `refund`. The store is the truth for accounts; tab balances live in `CreditsHub` and are swept after an hour idle.

### The catalogue (one table in `credits.rs`, prices in credits; 1 credit = 30 s)

| Offer | Cost | Target | Effect |
| --- | --- | --- | --- |
| Food / Potion / Gift | 5 / 8 / 10 | selected villager | item into their pack |
| Skeleton minion / rogue / warrior / mage | 20 / 30 / 40 / 50 | camera focus | one monster rises there |
| Feast | 80 | none | everyone's hunger filled |
| Healing | 100 | none | everyone healed |
| Raid | 150 | none | a wave of 4 from the graveyard |
| Necromancer | 250 | none | one at the graveyard, if none is abroad |

### Spending

`POST /api/credits/buy {viewer, offer, characterId?, x?, z?}` in new `api/credits.rs`:

1. Key from `ChatKey::of`; the buyer must be in the chat room (gives the name). Rate limit with the existing `RateLimiter` (burst 3, then one per 5 s).
2. Refuse when the world is paused.
3. Check against the sim under `core.read()` with the **same** function the sim uses when applying (`viewer_events::check`). Refusals are sentences ("Her pack is full", "Nothing can rise there", "Too many of the dead are abroad") and cost nothing.
4. Spend atomically; too few credits → 409.
5. Send `WorldInput::Command { command, source: "viewer" }`; refund if the send fails.
6. Record the purchase, add a chat line (new `ChatKind::Event`, via a new `ChatHub::announce`), answer with the new balance.

`GET /api/credits` returns `CreditsState {balance, cap, perMinute, offers}`. Presence and buy answers carry the balance, so no new stream event is needed.

### In the sim

- `commands.rs`: `Command::ViewerEvent { by: String, event: ViewerEvent }`, with `ViewerEvent = Gift{characterId,item} | Monster{kind,pos} | Raid | Necromancer | Heal | Feast`.
- New `systems/viewer_events.rs`:
  - `pub fn check(state, &event) -> Result<(), Refusal>`: character alive and pack below `ITEM_CAP`; `nearest_walkable(pos, 6)` exists; room under `MAX_MONSTERS`; no necromancer abroad.
  - `pub fn apply(ctx, by, event)`: re-runs `check`, silently drops what is no longer valid, cleans and caps `by` (24 UTF-16 units).
  - Gift follows `run_give` (`intents.rs:528-604`): item added, memory, stimulus, `Emote::Gift` (texts under "Powers and prayer").
  - Monsters use `spawn_monster`; raid points and jitter as `spawn_wave` (`director.rs:59-71`), RNG only through `ctx`.
  - Heal and Feast set hp / hunger for `living_ids`, with a shared memory.
  - Each writes one `ctx.chronicle(ChronicleKind::World, "{by} raised a skeleton warrior near {place_name_at(pos)}.", ids)`; large ones emit `SavePoint`.
- `state.rs`: `Monster.until_tick: Option<i64>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`, so old saves load and round-trip unchanged (no `SAVE_VERSION` bump).
- `monsters.rs` `update_monsters`: a monster with `until_tick` skips the daylight crumble and dies when the tick is reached. Bought monsters live 180 game minutes (3 real minutes at 1×). A bought necromancer may summon at any hour and its minions share its `until_tick` (check the summon gate at `monsters.rs:211-234` when implementing).

### Powers and prayer

Villagers treat each buyer as a named power that watches the valley. Three layers, so it works with and without a model:

1. **Scripted, at once (deterministic, no model needed).** New `data/prayers.rs` holds short line tables with a `{name}` slot; lines are picked with `ctx.pick` and spoken with `ctx.say` (`util.rs:126`).
   - Gift: the receiver gives thanks ("Thank you, Erik!").
   - Monster, raid, necromancer: up to two awake, outdoor villagers who can see it (`nearby_characters`, `util.rs:327`) cry out ("Spare us, Erik!"), loud.
   - Heal, feast: up to two villagers give thanks ("Bless you, Erik, lord of plenty!").
2. **What they know.** Memories and stimuli name the power: `ctx.remember(id, "Erik, one of the powers that watch the valley, put a potion in my pack.", …, MemoryKind::Event)` and `ctx.stimulate` with two new kinds, `StimulusKind::Favour` and `StimulusKind::Wrath` (urgency 1 and 2). These flow into the decide prompt through the existing recent-events list, so the model can pray, thank or curse in its own words. Witnesses of a spawn get the wrath memory; everyone gets it for a raid or a necromancer, and the favour memory for heal and feast.
3. **A lasting reputation.** `SimState.powers: IndexMap<String, Power { favours, wraths, last_tick }>` (serde default, skipped when empty, capped at 24 with the least recent dropped), updated on every event.
   - Scripted prayer: when a villager starts the existing `Pray` activity (chapel or shrine; hook near `intents.rs:168`) and powers are known, they sometimes say a prayer to one: thanks to the most generous, pleading to the most feared ("Erik, stay your hand tonight.").
   - Prompt: one sentence added to `WORLD` (`brain/src/prompts.rs:65-74`) saying unseen powers watch the valley and villagers may thank, plead with or pray to them by name; and one line in the decide situation listing known powers with a word each (generous, cruel, fickle). This is a deliberate prompt change: update `crates/brain/tests/fixtures/prompts.json` in the same commit and say so.

### Prayer pays

When enough villagers pray to a viewer, that viewer earns extra credits.

- **What counts as a prayer:** any scripted line above (thanks, pleas, chapel prayers), and any line a villager speaks that names a known power (checked in `ctx.say` against `state.powers`, whole-word, ignoring case), so the model's own prayers count too. Pleading counts like thanks: fear is worship as well.
- **Sim side:** a new `SimEvent::Prayer { power, character_id }`. It is not sent to browsers (`encode.rs` unchanged) and changes nothing in the sim, so determinism is untouched.
- **Server side:** the world task, where it already scans events for chronicle entries and save points (`world_task.rs:514-535`), passes prayers to `CreditsHub`. The hub keeps, per power and game day, the set of distinct villagers who prayed. At `PRAYERS_NEEDED = 3` distinct villagers the viewer gets `PRAYER_BONUS = 15` credits (up to the cap), at most once per game day per power. A chat `Event` line announces it ("The village prays to Erik: +15 credits").
- **Finding the viewer behind a name:** each purchase records the name used. For accounts it is stored in a `power_name` column on `credits` (lower-cased, looked up by name; the latest buyer to use a name owns it), so an account earns even while offline and across restarts. For tabs the map is in memory.
- **Why the limits:** three gifts of food cost 15 and can earn the 15 back once per game day (24 real minutes at 1×), so prayer rewards generosity and notoriety without becoming a credit pump. The tally is in memory and starts again after a restart.

Names: the sim accepts `by` only if it passes the chat name rules (letters, digits, space, `- _ ' .`, 2–24 UTF-16 units; `chat.rs` `validate_name`, mirrored in the sim); anything else becomes "a nameless power". That narrow alphabet and length is what keeps a viewer-chosen name from steering the model's prompt. A viewer who renames in chat is a new power to the villagers.

### Wire

`wire.rs`: `PresenceRequest`, `BuyRequest`, `CreditsBalance`, `CreditsState`, `CreditOffer`, `CreditTarget`, and `ChatKind::Event`; add them to the `decl!` list. Bump `PROTOCOL_VERSION` to 3 (old clients cannot show the new chat kind). Run `export-api`; run `export-frames` and expect no diff.

### Client

- `src/net/api.ts`: `credits()`, `presence()`, `buy()`.
- `src/net/presence.ts` (new): `Heartbeat` with an injected timer and `isActive`, so it is testable under vitest's node environment; started in `main.ts`, added to `__debug`.
- `src/ui/store.ts`: `ui.credits` signal, `Panel` gains `'shop'`, `UiActions.buy(offerId)`.
- `src/main.ts`: `actions.buy` fills in `ui.selectedId` and `view.rig.target` (x, z), since `ui` may not import `render`; errors go to `ui.notice`.
- `src/ui/Shop.tsx` (new): a `.modal > .panel.dialog` listing offers by group, greyed out when too dear, "select a villager first" for character offers. A credits `.pill` in `TopBar` (`App.tsx:30-52`) and a `menu-list` entry in `Mobile.tsx`. Hidden when `perMinute` is 0.
- `src/ui/Chat.tsx`: render `event` lines like join/leave notices; `addChat` does not count them as unread.

### Docs

- `README.md` settings table and a short "Credits" section; `docs/deploy.md` line for the two settings; AGENTS.md route list mention if needed.

## As built

Where the code differs from the design above:

- **Prayers are counted by their own task**, not the world task: `credits::spawn_prayers` listens to the sim's events, as the brain does, because the credits live on `AppState` and the world task does not see it.
- **A name the villagers could not say** leaves no power behind: the chronicle says "A nameless power", villagers say "lord", and no prayer is counted.
- **Blessings nobody needs are refused** (nobody hurt, nobody hungry), so credits are not spent on nothing.
- **Too few credits is 402**; other refusals are 409 with the reason as a sentence. The balance is checked before the rate limit, so only a purchase that would go through uses one of the three turns.
- **`GET /api/credits` takes `?viewer=<tab>`** and also answers `beatSeconds`; when credits are off it answers `perMinute: 0` and no offers, and the other two routes are 404.
- **The page reloads its credits** when the chat shows an event under its own name, which is how a prayer bonus shows up without waiting for the next report.
- **A power's name is matched in speech as whole words, whatever the case.** A viewer who takes a common word, or a villager's name, as a chat name would be "prayed to" by ordinary talk; the once-a-day bonus bounds what that earns.

## Gifts from an admin (added after review)

`GET /api/admin/credits` lists everyone with a stream open (the chat's members) with their chat name, balance, whether they have an account and whether their tab is reporting. `POST /api/admin/credits/grant {name, amount}` gives 1 to 10,000 credits to the viewer of that name, or to everyone listed when `name` is null. A gift is not held to the cap. The chat announces it, under the viewer's name or as "Everyone watching", and pages reload their balance when they see either.

## Order of work (tests first in each step)

1. Spec document.
2. Sim: `until_tick`, `ViewerEvent` command and system. Tests in new `crates/sim/tests/viewer_events.rs`, modelled on `tests/billboards.rs`: each event applies at the next tick; invalid input is ignored; caps hold; a bought monster survives daylight and crumbles on time; save round trip; determinism through a JSON round trip. `save_fixtures.rs` must still pass. Also: the receiver thanks the buyer by name; witnesses of a spawn cry out and get a `Wrath` stimulus naming them; `powers` counts and its cap; a praying villager addresses a known power; a bad name becomes "a nameless power". Brain: prompt fixture updated, and a test that known powers appear in the decide prompt.
3. Store: migration 3, `earn`/`spend`/`refund`/purchases, unit tests with `testing::store()`.
4. `CreditsHub` pacing (pure, takes `Instant`s; unit tests for too-fast, too-slow, cap, remainder) and config parsing tests. Prayer tally tests: three distinct villagers pay once, the same villager three times does not, a second bonus the same game day does not, an unknown name is ignored. Sim test: a spoken line naming a known power emits `SimEvent::Prayer`.
5. Routes and wire. Tests in `tests/http.rs` (tab balances, presence needs an open stream, buy refusals, CSRF) and `tests/auth.rs` (a signed-in `Role::Viewer` earns, buys, and the balance persists).
6. Client: heartbeat, store, shop, chat line; `src/net/presence.test.ts` and a store test.
7. Docs.

## Verification

- `pnpm typecheck && pnpm test`, then in `server/`: `cargo fmt --check && cargo clippy --all-targets --locked -- -D warnings && cargo test --locked`.
- By hand: `pnpm build`, then in `server/` run with `AUTH_MODE=off PUBLIC_ORIGIN=http://localhost:8080 STATIC_DIR=../dist DATA_DIR=<scratch> CREDITS_PER_MINUTE=60`. In the browser pane: watch the pill rise while focused and stop when the tab is hidden; buy a potion for a selected villager (inspector shows it, chat and chronicle name the buyer); buy a skeleton at the camera focus by day and see it rise, fight and crumble after about three minutes; confirm a too-dear offer is refused without charge. Check the speech bubbles: the receiver thanks the buyer by name and villagers near a spawn plead by name. A second run with `node tools/mock-llm.mjs` and `LLM_BASE_URL=http://localhost:8787/v1 LLM_MODEL=mock` confirms the brain log's decide prompt lists the known powers. Read state through `__debug` where the hidden pane throttles rendering.
