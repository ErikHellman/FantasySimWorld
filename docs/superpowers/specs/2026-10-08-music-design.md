# Music: singing and playing in public

October 2026. The first entertainment in the village: some villagers sing or play an instrument, mostly of an evening outside the tavern, and whoever is about comes to listen. This note records what was built and why; the code is `server/crates/sim/src/systems/music.rs`.

## What happens

- **Talent.** Whether a villager has a gift for music, and for what (voice, lute, fiddle, pipe or drum), follows from who they are: a trade, trait or story that speaks of singing, a lute or a drum settles it, and of everyone else about three in ten have a talent anyway, decided by a hash of their name. Nothing is drawn from the RNG for it, so a village lives exactly as it did before music until someone strikes up. Children have no talent yet. Each performance given makes the next a little better, up to a point (`Character.music`, left out of a save until they first perform).
- **Striking up.** A musician's routine puts a performance in the evening block now and then (more often for the sociable and the skilled), and less often in a free afternoon; one of their own accord an evening. The language model may also ask anyone to `perform`, musician or not; without a talent it is a thin voice. The performer walks to a spot of their own in front of the tavern (three times in four) or on the green, says a line, and plays for 25 to 45 game minutes. How well it goes is settled at the first note: skill, a little luck, less when drunk or worn out.
- **The audience.** Once a game minute everyone awake and outdoors within earshot who is idle, waiting or at leisure or a meal counts as a listener; those on their feet with nothing the model asked of them walk over (children almost always) and stand in an arc before the performer (`IntentAction::Listen`), while the drinkers listen from their stools. Nobody at work, on watch, in a fight, at a fire or at a ceremony is drawn away, and nobody can start a chat with someone mid-song. Every few minutes one listener shows what they think: a laugh, a heart, a cheer and a line for a good tune; a heckle from the hot-tempered, or their back, for a poor one.
- **The last note.** If the music lasted at least eight minutes, everyone who heard it is affected. A good tune (quality 0.45 and up) heartens its listeners for three hours or more (`Character.heartened_until_tick`): they tire a quarter slower, heal a little faster, find a touch more nerve when the dead come, feel less lonely, forgive a little of every grievance, and think better of the performer. A passable one is remembered mildly; a poor one costs the performer a little affection. The performer, the listeners and the chronicle all remember how it went, and the performer's stimulus tells the model.
- **Interruptions.** A performer drawn away (a fight, the alarm, a chat the model started) breaks off; the audience is released, and the music does not pick up again afterwards.

## What viewers see

A note (🎵) over the performer, renewed while they play, and the usual emotes over the audience. The status line reads "playing the lute outside The Gilded Tankard" or "listening to Torvald sing", with "(in good spirits)" on the heartened. There is no instrument model in the KayKit collection, so a player holds nothing and uses the holding animation; a singer stands and sways. While a performance is in view, the browser's background music switches to the minstrel pieces (`src/audio/mood.ts`), standing in for what is played; a real tune heard from the performer's position would need a licensed recording per instrument and is left for later.

## Where it lives

- `fsw-world`: `ActivityId::Perform`, offered to the model like any other activity (not to children).
- `fsw-sim`: `systems/music.rs` (talent, the performance, the audience, the perks), `SimState.performances` (the record of each performance, removed when the music stops), `IntentAction::Listen`, `StimulusKind::Music`, `Emote::Music`; `update_music` runs right after the intents each tick.
- `fsw-brain`: the activity list and the character header name the talent ("You play the lute well, and now and then do so in public"); `sing`, `song`, `music` and `play music` resolve to `perform`.
- The wire: one new emote (protocol 8).
