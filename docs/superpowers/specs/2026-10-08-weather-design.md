# Weather

The valley has weather: clear skies, rain, wind and hail. Each villager takes it their own way (some love the rain, some hate it), it shows in their mood and their needs, those who hate the rain go indoors when it pours, and hail sends everyone running for cover, because it hurts. Rain helps keep a fire down and wind fans it.

## What happens

1. **The sky keeps its own course.** The weather comes in spells: clear for 3 to 10 game hours, rain for 1 to 5, wind for 2 to 6, hail for 10 to 30 minutes. When a spell ends the next is rolled from what came before (after clear: clear 55%, rain 22%, wind 20%, hail 3%; after rain: clear 45, rain 30, wind 15, hail 10; after wind: clear 45, rain 30, wind 20, hail 5; after hail: rain 60, clear 30, wind 10). Each spell has a strength (rain and hail 0.4 to 1, wind 0.5 to 1) and a wind with it (0 to 0.2 under a clear sky, up to 0.8 in hail) blowing one way. A new world is clear for its first six hours. Hail is rolled as rain while a wedding or funeral is arranged.
2. **It comes on and dies away.** Strength moves toward the spell's a little every game minute (5% a minute; hail 20%), and a change of kind lets the old weather die away before the new one starts. The chronicle notes each change ("Rain sweeps in over the valley.").
3. **Each villager takes it their own way.** Words in their trade, traits, goals or story give it away: farmers, gardeners, fishers, poets and the melancholy love the rain; merchants, tailors, the vain, the proud and the frail hate it; sailors, millers and the wild or restless love a wind, the timid, the anxious and scholars hate it. For everyone else their name decides: about a third love the rain and a third hate it, a quarter love the wind and a quarter hate it. Everyone but the rain's lovers likes a fair day, though only half as much. Nobody likes hail.
4. **Mood.** Weather a villager loves, at full strength, slows their tiredness and loneliness by 15%; weather they hate speeds both by 15%. A fair day counts for half, and a fair night not at all. The inspector shows it ("glad of the rain", "glum in the rain", "enjoying the wind", "braced against the wind", "pelted by hail"), and the model is told ("Rain is pouring down. You love the rain.").
5. **When the weather turns** the village notices one by one over six minutes: everyone who feels strongly is told, those outdoors laugh or scowl, and one in three says so ("Ah, rain at last!", "Rain again. Wonderful.").
6. **Rain-haters go in.** While rain falls at half strength or more, those who hate it leave work and leisure out in the open and go home (or to the tavern, if they have no home) a few at a time, and their routine keeps them in until it eases below 0.3, they get hungry, they are worn out or it is bedtime. Eating, sleeping and keeping watch go on whatever the weather. Villagers doing what the model chose are only told; the model decides.
7. **Hail sends everyone in.** Everyone out in hail (children, those sleeping rough, those whose minds chose otherwise) runs home or to the tavern, a few each tick, and stays in until it stops. Only fighters and the bucket chain stay out. The model cannot call them out; only a fight or flight comes first. Anyone still out has, each game minute, up to a one-in-four chance of being struck for 1 to 3 hit points, but hail never takes anyone below 5: it hurts, it does not kill.
8. **Fires.** A fire's growth is scaled by `1 + 0.5 × wind` and by `1 − 0.6 × wet`, where wet is the strength of rain or hail, and wet weather also knocks 0.8 off it each minute. A downpour drowns a young fire by itself; drizzle slows one; a gale makes it harder to save. A fire the rain puts out is chronicled as such.

## Measured

Not yet measured with the soak (`cargo run --release -p fsw-sim --bin soak -- 60 <seed> --log`). The tests in `server/crates/sim/tests/weather.rs` check that thirty days of one seed bring all four kinds, rain at least fifteen times, hail at most fifteen times and never for more than 45 minutes; that a downpour drowns a young fire within two hours; and that hail sends at least three in four villagers indoors within twenty minutes without killing anyone.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `SimState.weather` (`Weather { kind, intensity, target, wind, windTarget, windDir, sinceTick, untilTick, next }`), left out of a save while it is the default, so older saves load and the fixtures are unchanged. `IntentAction::Shelter` and `StimulusKind::Weather` are new.
- **The system** (`server/crates/sim/src/systems/weather.rs`): the schedule (`roll`), tastes (`taste`, after `music::talent`), mood (`mood`, `feeling_words`), going in (`run_shelter`), the hail muster and strikes, and the rain-haters. Its constants are at the top of the file; the lines people say are in `data/weather.rs`. `update_weather` runs first each tick, before the director. The schedule is a hash of the seed and the tick, never the RNG, so a seed has the same weather whatever happens in it; only hail strikes draw from the RNG.
- **Elsewhere in the sim:** `needs.rs` (the mood's pace), `routine.rs` (rain-haters choose to stay in), `fire.rs` (growth and drowning), `describe.rs` (status), `sim.rs` (sheltering from hail binds like the bucket chain).
- **The brain** (`server/crates/brain`): one line in the prompt header while it rains, blows or hails, with how the reader feels about it; nothing on a fair day. Villagers sheltering from hail are not asked to decide anything. The prompt fixture was regenerated for this.
- **The wire** (`wire.rs`, protocol version 11): `Snapshot.weather` and the `weather` event (`{ kind, intensity, wind, windDir }`, strength and wind in steps of 5, direction in steps of 15 degrees), found by comparing state as the economy is.
- **The client:** `src/render/weather.ts` draws rain as slanted streaks and hail as bouncing pellets in a box around where the camera looks (one instanced mesh each; `rainDrops` in `quality.ts` sets how many). `lighting.ts` dims the sun, greys the light and sky and draws fog in as clouds come (the fog is always there, far off under a clear sky, so no shaders change). On the high tier the trees bend with the wind (`windSway`); smoke from a fire drifts with it. The ambience gains rain and hail loops (both cut from haruta's CC0 "light rain and heavy rain"; no free recording of hail turned up, so the hail is its downpour pitched up and stripped of its rumble, in `tools/audio/sources.ts`), the wind loop rises with the wind, and birds and the village murmur fall quiet in rain. The top bar shows the weather beside the clock, and a banner says when hail is falling.

The nav grid, `worldHash` and the golden fixtures do not change.

## Left out on purpose

- No seasons, snow, thunder and lightning, puddles or wet clothes.
- Tastes are not written by the model, and are not shown in the inspector except as the mood of the moment.
- Rain does not stop the dead or slow anyone down, and hail does not hurt monsters.
- Sound is not muffled indoors, and the weather does not follow the camera into a house.
- Rain is not a reason for the model to be asked to decide; it is told in the prompt and through a quiet stimulus.
