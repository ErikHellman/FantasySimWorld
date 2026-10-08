# In the way: villagers who block one another

October 2026. A villager held up by someone standing in their path stops and says so, and the one in the way steps aside. This note records what was built and why; the code is `server/crates/sim/src/systems/blocking.rs`.

## What happens

- **Being held up.** Bodies are nudged apart only when they move (`movement::separate`), so someone standing still squarely on a walker's route holds the walker up: the walk forward and the push back cancel out. A walker counts as held up after 1.5 s with another villager touching them and straight ahead. Someone walking the same way does not count, and nor does the person the walker is on their way to. Nobody stops for this in a fight, while fleeing or stalking, while carrying water to a fire or running after a child. The count is kept in `Act.stuck`, which the fights keep for themselves while one is on.
- **Saying so.** The walker turns to the one in the way and says a line to them by name (`data/blocking.rs`). How cross they are depends on their temper and spite, any grievance against the other, whether they are drunk, whether they count the other as a rival or enemy, whether the same person held them up in the last three game hours, and any fondness for them, plus a little chance. Most villagers ask politely. The cross speak curtly, and the angriest shout with the anger emote. A child is at most curt.
- **Making way.** If the one in the way is standing idle (not seated, at a work spot, talking, fighting, asleep or indoors), they step 1.4 units to one side of the walker's line and take up what they were doing (`Character.sidestep`). If they cannot, the walker brushes past them for 3 s without being pushed apart (`Character.passing`). If the same person holds them up again within 20 game minutes, the walker brushes past without a word. Anyone asleep is passed in silence.
- **Hard feelings.** A curt or angry word adds a few points of grievance both ways (more for the hot-tempered and for a repeat offence). A shout also costs a little affection on both sides. Each side keeps a minor memory of it. A hot-tempered one in the way may answer back. From there the hostility ladder that already exists takes over: a fresh offence makes a confrontation in the routine more likely, so a pair who keep getting in each other's way can end up quarrelling. A single block never starts a fight.

## What viewers see

Speech bubbles and the anger emote, as for any other line; the one in the way walks a step aside. There is nothing new on the wire.

## Where it lives

- `fsw-sim`: `systems/blocking.rs` (`update_blocking`, run after perception and before separation each tick, and `brushing_past`, which `separate` asks), `data/blocking.rs` (the lines). It adds `Relationship.last_blocked_tick`, `Character.passing` and `Character.sidestep`, all left out of a save while unset.
- In a typical village the lines come up about ten to fifteen times a game day, nearly all of them polite.

## Left out

- The language model does not write these lines: they need to come at once, and the moment is too small to spend a request on. The memories it leaves do reach the model's prompts.
- Villagers do not route around someone standing still; the nav grid knows nothing of people, and stepping aside or brushing past covers it.
