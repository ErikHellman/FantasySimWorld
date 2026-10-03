# Billboards

Five large billboards stand around the village. For now they show funny catch-phrases and mock-motivational slogans; later they are meant to carry real adverts. Villagers sometimes walk up to one, read it, say what they think of it and react to it.

## What was decided

- **Content lives in the sim.** Villagers can only react to text the sim knows, so what a board shows is part of the world. Without a notice, a board shows the day's slogan from a built-in deck of 40, worked out from the seed, the day and the board's index: it draws nothing from the RNG and nothing is saved for it. Text put up from outside (an admin today, an advertiser later) is a `Command::SetBillboard`, applied at a tick boundary and saved in `SimState.billboards`. A future ad feed goes through the same command and the same cleaning.
- **Reactions work without a model.** Every slogan has a tone (uplifting, absurd, pushy, ominous, wholesome). The stock reaction comes from the tone and the reader's temper, bravery and sociability, with no randomness: a hot temper scoffs at uplifting slogans and is annoyed by pushy ones, the timid are unsettled by ominous ones and the brave laugh at them. A stock line for that reaction is said aloud.
- **With a model**, reading a board raises `SimEvent::BillboardRead`; the brain asks `react_to_billboard` (a reaction and an optional line) when it has room to spare (`Tier::Idle`, not backlogged). The reader stands at the board for up to the model's usual wait (`script_wait_ticks`, capped at 30 s of ticks); an answer in time replaces the stock reaction, otherwise the stock one comes at the end. The prompt frames the text as paint on wood, which cannot ask anything of the reader.
- **Who reads, and when.** Only the routine sends people to boards (`ActivityId::ReadBoard` is not offered to the model, so the prompts' activity lists are unchanged). When someone with nothing pressing picks their next thing, a board they have not read within 60 m (in a leisure block) or 20 m (otherwise) with a free reading spot gives a 10% or 3% chance of going to read it. Each villager remembers a hash of the last text read at each board and never reads the same text there twice. Over five days on three seeds that is about 15 reads a day for 12 villagers.
- **Effects of a reaction:** a speech bubble, an emote (laugh, heart, thinking, anger, alert), a small change to the social need (and energy for love and unease), a low-importance memory quoting the board ("Read the billboard at the market: “…” It made me laugh."), and the model's thought if it gave one. The memories reach later prompts, so villagers can bring it up.

## The pieces

- **World** (both twins, `src/world` and `server/crates/world`): `PlaceDef.billboards` with an id, a local position and a rotation. Each board adds a box obstacle (3.4 × 0.5 m) and two `read_board` slots in front of it, appended after the authored slots so existing slot ids stay the same. Boards: the green, the market, the tavern, the training yard and the west road. No place was added, so scatter and the prompts' place list are unchanged.
- **Sim**: `data/billboards.rs` (the deck and the stock lines), `systems/billboards.rs` (what a board shows, cleaning text, the stock reaction, reacting, wanting to read), the `ReadBoard` activity in `intents.rs` and `routine.rs`, and the two commands in `sim.rs`.
- **Brain**: `RequestKind::Billboard`, `billboard_prompt`, the `react_to_billboard` tool.
- **Server**: `Snapshot.billboards`, `WireEvent::Billboard` (sent by the tracker whenever what a board shows changes, at midnight or after an admin edit), and `POST /api/admin/billboards {id, text?, tone?}` (admins only, JSON, CSRF-checked; text is trimmed, whitespace collapsed, control characters refused, at most 90 UTF-16 units; no text returns the board to the slogans).
- **Client**: `render/billboards.ts` builds each board from three.js boxes and a plane with a canvas texture (no new model, so no asset pipeline run), painted by tone and laid out by `render/billboardText.ts`. The admin panel has a Billboards section, and the help mentions them.

## Consequences

- The board obstacles change the nav grid (a new `worldHash`) and the extra dice rolls change how every world plays out. 66 of the 69 cases in `server/crates/brain/tests/fixtures/prompts.json` now carry the Rust sim's text; their fixed wording (system text, place and activity lists, tool lists) did not change.
- Old saves load unchanged (both new fields are optional and left out when empty).
- No `PROTOCOL_VERSION` bump: older clients ignore the new event and field, and the changed world hash already tells a stale tab to reload.

## Later

- Images on a board (an `imageUrl` next to the text) and an ad feed that sends `SetBillboard` on a schedule.
- Letting the model choose to read a board, if it seems worth the extra prompt text.
