# Ambitions

October 2026. Villagers met their needs and followed their routines, but nobody wanted anything from life, so there was little for a viewer to root for over days. Now every grown villager sets their heart on one long goal, in their own words, shown on their page with how far along they are. The chronicle says when one comes true, and when someone gives up on theirs. The village is still watched, not steered: viewers can only see an ambition, never set or help it.

## What happens

1. **A newcomer brings one.** The casting model writes an ambition with the rest of a villager (`create_villager` has a required `ambition`: one of five kinds and a sentence in their voice). The kinds are:
   - **wealth**: put by 60 gold on top of what they had;
   - **love**: be married;
   - **family**: have a child of their own (one more than they had);
   - **glory**: fell three of the village's enemies (the dead or raiders);
   - **friends**: have two close friends.
2. **Or the sim gives one.** A villager the model gave none (no model, an answer without one, a world saved before ambitions, a child come of age) gets one from their temperament: the bold (bravery over 0.75) want glory, the gregarious (sociability over 0.75) friends, and the rest a fortune, a marriage or a child, picked by a hash of the seed and their name. This draws nothing from the RNG.
3. **Already theirs.** Marriage and friends count from nothing, so one made before counts too. Someone whose ambition is already met on the day they set out (a founder married by a founding bond) takes a fortune instead.
4. **The sim watches.** Once a game hour it checks everyone grown and still in the village:
   - an ambition that has come true is **won**: the chronicle says "Name has done what they set out to do: “…”", they keep a memory of it (importance 7) and show a heart;
   - one not won after 30 days is **given up**: the chronicle says "Name has given up on it: “…”" and they keep a memory of that (importance 6).

   Either way it stays on their page, and they set out on nothing new.
5. **The model hears of it.** Every prompt about a grown villager has one more header line: what they want most, in their words, and how far along they are ("23 of 60 gold put by.", "Courting Mira; not married yet."), or that they achieved or gave it up. When a prompt is over its budget, the least important section is dropped as before.
6. **Viewers see it.** A villager's page shows the ambition above their wants: an icon for the kind, their words, a progress bar and the same line of progress, or the day it was won or given up (`CharacterDetail.ambition`).

## Where it lives

- `server/crates/sim/src/systems/ambitions.rs`: `stock`, `begin`, `progress`, `progress_words`, `ambition_line` and `update_ambitions`. `Ambition`, `AmbitionKind`, `AmbitionDraft` and `AmbitionOutcome` in `state.rs`; `Character.ambition`, and `NewVillager.ambition` for the model's draft. `arrivals::make_character` sets one out for every grown newcomer and founder.
- `server/crates/brain/src/casting.rs`: the `ambition` property of `create_villager` and its reading. `prompts.rs`: the line in `header`.
- `server/crates/server/src/digest.rs` and `AmbitionView` in `wire.rs`. No protocol bump: the field is optional.
- `src/ui/Inspector.tsx`: `AmbitionCard`.
- `tools/mock-llm.mjs` gives every invented villager an ambition.
- Tests: `server/crates/sim/tests/ambitions.rs` and the casting tests in `brain/src/casting.rs`.

## Left out on purpose

- **No pursuit.** An ambition does not yet change what anyone does; it shows in their talk through the prompt line, and comes true through the life they already lead. Steering plans toward it is a later step once it is clear which goals the village can reach.
- **One ambition a lifetime.** Someone who wins or gives up keeps that as their story rather than drawing a new goal, so a page reads as a life, not a list of chores.
- **Viewers cannot help.** No cheering, betting or nudging: nothing a viewer does reaches an ambition.
