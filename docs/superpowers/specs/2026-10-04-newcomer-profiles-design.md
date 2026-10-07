# Newcomers written by the model

> **Built on 2026-10-07, with these differences.** There is no authored cast any more: a new world founds itself the same way, one `Command::Settle` per villager the model invents (`fsw_brain::casting`, `server/crates/server/src/casting_task.rs`), and the authored people are in the prompt for flavour only. The casting task calls the model with its own client, outside the brain's queue, governor and decision log. The tool is `create_villager`; a `link` is a `bonds` list of up to two, and the model may not give `murderous` or `occultist`. The sim asks with `SimEvent::NewcomerWanted` and waits six game hours (`NEWCOMER_PATIENCE_MINUTES`) for a `NewcomerDraft`; `NewcomerLocal` tells it none is coming, and then or at the deadline it invents the newcomer itself from `data/names.rs` instead of a wanderer. The rest of this page is the design as first written.

When the eleven authored newcomers have all arrived, the village today fills its empty beds with wanderers made from lists: a class, a name, three traits and the same backstory for every one of them. This lets the language model write each newcomer instead, with the village in mind: who died and how, who misses them, which work nobody does any more. Some come as the sister of the woman the dead killed last week, or the old comrade of the guard who fell at the gate. Without a model, wanderers come as before.

## What happens

1. **An arrival is due.** As today, a death or a short village schedules the next arrival half a day to two days ahead (`director.rs`, `death.rs`). Nothing changes while authored newcomers remain.
2. **The model is asked early.** Once the authored list is used up, an arrival is scheduled and no draft is waiting, the brain sends one low-priority request (the same rank as a night's reflection). It has hours of game time to answer.
3. **It is told about the village.** The prompt lists the living (first name, occupation, class), the dead of the last ten days (name, day, cause, and the living who were close to them), work that is short (for example fewer than two who can stand the night watch), the house with a free bed, and the day. It asks for one person who would plausibly come up the west road now, and suggests kin or friends of the dead.
4. **It answers with a person.** The `newcomer` tool has a closed schema:
   - `firstName`, `lastName`
   - `pronouns`: she/her, he/him or they/them
   - `age`: 18 to 70
   - `class`: one of the classes that has a model
   - `occupation` (up to 40 characters), `backstory` (600), `speechStyle` (120), `timbre` (40)
   - `traits`: two to four, each up to 24
   - `goals`: two or three, each up to 80
   - `secret`: optional, up to 200
   - `temper`, `bravery`, `sociability`: 0 to 1
   - `quirk`: optional, one of the quirks except `murderous`
   - `reason`: why they came, up to 200
   - `link`: optional `{to, relation}`, where `to` names a living or dead villager and `relation` is sibling, child, parent, spouse, friend or comrade (a spouse arrives married, if the other is free; see [the weddings and funerals design](2026-10-08-weddings-funerals-design.md))
5. **The brain checks it.** Every text is cleaned (`clean_text`, the same rules as billboards) and cut to its limit. The first name must be new: not among the living, the dead or the authored cast. The name must have a first and a last part. `link.to` must resolve to someone. An answer that fails is dropped and logged; the arrival then uses a wanderer. A good answer becomes a `NewcomerDraft` command.
6. **The sim holds the draft.** The draft waits in `Director.newcomerDraft` until the arrival. A second draft is not asked for while one waits.
7. **The newcomer arrives.** When the arrival comes, the sim checks the draft again (the name may have been taken since, the person it links to may have changed) and builds the villager from it. The look comes from the class, with the same random texture choice a wanderer gets. Jobs and hobbies come from the class. Spite, allure, shyness and looks start at 0, as for wanderers. Voice pitch and rate are drawn as for wanderers. The draft is cleared.
8. **A link to the dead matters.** A newcomer who comes for someone who died:
   - remembers why (importance 8): "I came because my brother Halvard died here."
   - starts as family or friend to those who were close to the dead, and those people remember meeting them
   - is named with the reason in the chronicle: "Ingrid, a tanner, arrived by the west road, looking for her brother's grave."

   A link to the living gives the bond and memories on both sides.

## Where it lives

- **The command** (`server/crates/sim/src/commands.rs`, `sim.rs`): `Command::NewcomerDraft(Box<NewcomerDraft>)`. It is applied at a tick boundary and recorded like every other command, so replay is unchanged.
- **The state** (`server/crates/sim/src/state.rs`): `Director.newcomer_draft: Option<NewcomerDraft>`, left out of a save when empty. Older saves and the `export-day-*` fixtures load unchanged.
- **Arrivals** (`server/crates/sim/src/systems/arrivals.rs`): `arrive()` builds a `CastEntry` from the draft and goes through `make_character`, as authored newcomers do. The link's bonds and memories reuse the existing bond and memory helpers.
- **The brain** (`server/crates/brain`): request kind `Newcomer` (`brain.rs`, also in `wire::RequestKind` and `brain_task.rs`), `prompts.rs::newcomer_prompt`, the `newcomer` tool (`tools.rs`), and `validate.rs::newcomer`.
- **The wire**: only `RequestKind` grows, for the admin's decision log. `export-api` regenerates `src/generated/api.ts`. The inspector already shows the profile.

## Tests

- `server/crates/sim/tests/newcomers.rs` (new):
  - a held draft becomes the next arrival, with its name, class, profile and quirk
  - a draft whose first name was taken meanwhile gives a wanderer
  - with no draft, a wanderer arrives as today
  - a link to the dead gives the memories and bonds
  - a save holding a draft round-trips, and the old fixtures still load
- `server/crates/brain/tests/brain.rs`: a good answer gives a `NewcomerDraft`; a bad class, a missing field, a taken or authored first name, an unknown link and over-long text are refused; no second request is sent while a draft waits.
- `brain/tests/prompts.rs` gains the newcomer prompt for the fixture world. That is a deliberate change to `prompts.json`, and the commit says so. The existing prompts do not change.

## Left out on purpose

- The model does not choose the look, the home or the arrival time: those stay with the sim.
- No new authored newcomers, and the authored ones keep their order.
- No murderous newcomers from the model. The authored cast holds the village's dark secrets.
- No portrait or appearance text beyond the class model and its texture.
