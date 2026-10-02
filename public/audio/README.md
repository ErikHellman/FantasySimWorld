# Sound files

These files are built by `pnpm audio` (`tools/audio/build-audio.ts`) from the
sources in `tools/audio/sources.ts`, and are credited in [CREDITS.md](CREDITS.md).
To use your own sound instead, replace the slot's file here, as `.ogg`, `.mp3`
or `.wav` (the mixer tries them in that order). A slot with no file is silent,
and the settings panel in the game lists which were found. Ambient loops ship
as Ogg Opus with an MP3 fallback for browsers that cannot decode Ogg.

Ambient slots are seamless loops; the mixer fades them in and out by where the
camera is looking, how far it is zoomed out, the time of day and what the
villagers are doing.

| File name        | What it is                                             |
| ---------------- | ------------------------------------------------------ |
| `birds_day`      | Birdsong, daytime loop                                 |
| `wind`           | Soft wind loop, heard most when zoomed out             |
| `forest`         | Forest loop (leaves, distant birds), near the trees    |
| `river`          | Running water loop, near the river                     |
| `shore`          | Lapping water loop, near the lake                      |
| `crickets_night` | Crickets and night insects loop                        |
| `village_day`    | Village murmur loop (voices, hens, a cart)             |
| `tavern`         | Tavern chatter loop, when people are at the tables     |
| `smithy`         | Hammer on anvil loop, when someone is at the forge     |
| `campfire`       | Crackling fire loop, at the forest glade               |

Effect slots are short one-shots, played with a little random pitch variation.

| File name        | When it plays                                |
| ---------------- | -------------------------------------------- |
| `sword_hit`      | A weapon striking                            |
| `arrow_shot`     | A bow or crossbow loosing                    |
| `magic_cast`     | A spell being cast                           |
| `hurt`           | A villager being hit                         |
| `death`          | A villager dying                             |
| `monster_spawn`  | The dead clawing out of the ground           |
| `monster_death`  | A skeleton collapsing                        |
| `alarm`          | The alarm being raised (a bell or horn)      |

## Music

`music/<id>.mp3` is the background music, streamed rather than loaded up
front. The tracks and the moods they play in are listed in
`src/audio/musicTracks.ts`. The mood follows what the viewer is watching
(`src/audio/mood.ts`): day, evening or night by the clock; tension while the
alarm is up or monsters are near the camera; battle while blows land in view or
the selected villager is fighting; grief after a villager dies in view. The
stings `sting_victory` and `sting_defeat` play when a fight is won or a
villager is lost.
