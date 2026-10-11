# Feuds and love triangles

October 2026. Grievance already climbed a ladder of rungs (tension, grudge, feud, blood feud), shown on a villager's page. But apart from hostile talk and the odd fist fight, nothing marked the climb out in the village. Someone who lost their love to another took it hard and then did nothing. Now each rung shows in what villagers do to each other, and an admirer left out speaks up. Everything comes from what already drives the village: viewers only watch.

## What happens

1. **A snub (tension, grievance 20 or more).** Near the one they resent, a villager turns them away with a cold word ("Walk on, Mira."). It stings the other a little and vents the snubber a little, so two who keep snubbing each other do not climb the ladder on snubs alone. The first snub between two villagers goes in the chronicle; later ones do not.
2. **A row (a grudge, 40 or more).** With at least two others about, the two have a shouting match instead. Each says a line aloud, everyone about remembers it, and the chronicle says how many were watching. It vents the one who started it and stings the other.
3. **At a feud (65 or more), it depends on temperament.**
   - **Hot-headed** (temper over 0.5 or bravery over 0.6): they challenge the other to settle it with their fists before at least two onlookers, once a fist fight is open to them and neither is in the middle of work, a fire, an errand or sheltering. The brawl runs as any brawl does, and its end has its own chronicle line, memories and witnesses.
   - **Cool-headed**: they wait until they are at the other's door with the other well away and nobody awake within twelve metres. Then they slip in and take 3 to 10 gold (never more than the other has). The chronicle tells viewers who did it. The one robbed finds the coin gone and blames whoever they resent most, who is not always the thief.
4. **A jilted admirer speaks up.** Someone drawn to a villager (attraction 40 or more) who is with another tells them so. They must be unattached and not family, and the partner must be at least fifteen metres away.
   - The one they love weighs it as affection plus attraction for each, with a spouse counting 40 more. Nearly always they turn the admirer away. Each refusal cools the admirer by 12 and turns some of it against the partner, so the admirer gives up after a refusal or two, or starts a feud with the partner.
   - If they feel more for the admirer, they leave their partner, calling off any wedding planned. The two fall in love, and the one left behind holds a heavy grievance against the newcomer to their love.
   - A villager says so at most once every two days to the same person.
5. **At the wedding.** When the couple are pronounced married, a guest still sweet on one of them does not cheer. They say "I can't watch this.", their heart breaks, and the chronicle notes it. Their grievance against the one who was chosen grows.
6. **When.** Once a game minute, each free grown-up with a grievance or an unwanted love may act toward that person:
   - Each pair gets one moment in every 20 game minutes, set by a hash of the seed, the two of them and the stretch, so nothing draws from the RNG.
   - After a scene, the one who made it leaves that person be for half a day.
   - Nobody acts while the village is being founded, and children take no part.

Viewers see all of it in speech bubbles and the chronicle, so it feeds the Herald. A challenge becomes a brawl, which the Auto camera already follows.

## Where it lives

- `server/crates/sim/src/systems/feuds.rs`:
  - `update_feuds`, `confront`, `snub`, `row`, `challenge`, `can_burgle` and `burgle`;
  - `declare`, for the admirer;
  - `heartbroken_at` and `watch_them_wed`, for the wedding, called from `ceremonies::rite_step`.
- `Relationship.scene_tick` in `state.rs`. It is left out of saves until it is set, so older saves load unchanged.
- Tests: `server/crates/sim/tests/feuds.rs`.

## Left out on purpose

- **No formal duels on the green.** A challenge is fought where it is made. A booked duel with the village gathered is a ceremony of its own, for later if the fist fights prove too small.
- **Sabotage is only theft.** Smashing what someone built, or spoiling their work, would need things that can be damaged without being destroyed; the dynamic world only knows standing and gone.
- **The model is not told about scenes directly.** It hears of them through the memories they leave, as it does everything else.
- **Nothing on the page is new.** The rung was already shown beside each relationship; the scenes are in the chronicle.
