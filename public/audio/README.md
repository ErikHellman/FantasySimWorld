# Sound files

Fantasy Sim World has an ambient sound mixer but ships no audio. Put your own
files in this folder, named after the slot, as `.ogg`, `.mp3` or `.wav`
(for example `birds_day.ogg`). A slot with no file is silent. The settings
panel in the game lists which slots were found.

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
