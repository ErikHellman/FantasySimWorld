# Believers and sceptics

October 2026. The billboards already hint, now and then, that someone beyond the sky is watching the village, which is the viewers. Before this, a reader was shocked, intrigued or angry, remembered it and moved on, so the hints never added up to anything. Now they build into lasting stances that viewers can see on a villager's page and hear in the village. The believers can also feel roughly how many people are watching. The village is still watched, not steered: the viewer count is the only thing about viewers that reaches the sim, and it only changes what believers say.

## What happens

1. **A stance settles.** Each hint a villager reads counts toward one of three things:
   - shocked or intrigued counts as awe;
   - angry counts as anger;
   - anything else the model chooses (a laugh, a scoff) counts as doubt.

   After two hints they settle on a stance for good, in this order of checks:
   - **sceptic**, if doubt is at least as much as awe and anger together, or if they are bold (bravery over 0.75) and level-headed (temper 0.6 or less), whatever they felt at first;
   - otherwise **defiant**, if anger outweighs awe;
   - otherwise **believer**.

   The chronicle notes it, and they keep a memory of it (importance 5, so it comes up in talk).
2. **They say so.** In the daytime (7:00 to 20:00), believers and the defiant speak to the sky about once every three hours. Each picks their own moment in each three-hour stretch, set by a hash of the seed, who they are and the stretch. They only speak when out, awake and free: not talking, fighting, at a ceremony or performing, and with nothing queued.
   - Believers say things like "I know you're up there."
   - The defiant shout things like "Stop staring at us!"
   - The nearest sceptic within ten metres who is also free answers ("There's nobody up there, you fool.").
   - Sceptics never start it.
3. **The crowd.** Once a quarter of a second, the digest task buckets the server's viewer count into four steps: nobody, one or two, three to five, six or more. When the step changes it sends `Command::Audience`.
   - A new step is sent once, not again while it waits to be applied, and through the same queue as every other command.
   - When a crowd gathers (the top step) or everyone leaves, the first free believer or defiant villager says so ("The sky feels crowded today.", "They've gone quiet up there."), and the chronicle notes it. This happens at most once a game day.
   - Between those moments, half of what a believer or defiant villager says to the sky reflects the crowd, when there is a crowd or nobody to feel.
4. **The model hears of it.** A villager with a stance gets one more line in the header of every prompt. It names their stance and, for believers and the defiant, whether the sky feels crowded or quiet. The model can then let it colour their talk.
5. **Viewers see it.** A villager's page shows their stance under the secret line, if they have one (`CharacterDetail.sky`).

## Where it lives

- `server/crates/sim/src/systems/sky.rs`: `took_hint` (called from `billboards::react` for a board that hints), `settle`, `update_sky`, `set_audience`, `crowd_of` and `stance_line`. `SkyView` on `Character`, `Audience` on `SimState`, and `Command::Audience`.
- `server/crates/server/src/digest.rs`: `tell_the_crowd`, and the `sky` field on a character's page. `SkyStance` is in `wire.rs`.
- `server/crates/brain/src/prompts.rs`: the stance line in `header`.
- `src/ui/Inspector.tsx`: the line on the page.
- Tests: `server/crates/sim/tests/sky.rs`.

## Left out on purpose

- **The exact viewer count stays with the server.** The sim gets four rough steps, so a world's history does not hang on every reload of a browser. Each change in step is one command in the record.
- **A stance never changes.** It keeps a believer a believer, which is what makes them recognisable; a later stage could let something dramatic shake one.
- **No sermons or schisms.** A devout villager preaching against the watchers, or believers gathering, is left for later; for now it is single lines and answers.
- **Viewers cannot pick who believes.** Nothing a viewer does reaches a stance; only the billboards' hints and the villager's own nature do.
