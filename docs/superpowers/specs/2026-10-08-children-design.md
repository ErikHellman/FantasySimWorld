# Children

The village has a life of its own: lovers have children, children play, explore and get into trouble, and after about a month of sim time they grow into adults who take their place among the villagers. Five children live in the village from the first day.

## What happens

1. **Who they are.** A child is a villager like any other, with a persona, a mind the model speaks for, needs, memories and relationships, but no job. Their `occupation` is "child", their age runs from 5 to 15, and they have half the hit points of the class they will grow into. When the founding of a village ends, however its thirteen adults were invented, the sim adds four or five children of its own from the name and trait lists: couples among the founders get the first, single founders the rest, and the last is a foundling of the glade the whole village raises. Each has kin bonds both ways with whoever raises them, and lives in that parent's home. (The hand-written test cast brings five of its own: Pip Vane, Aldric's son; Tansy Stonefist, the daughter of the brother Garrick lost, raised by her uncle; Finn Thornwood, Kael's son; Hollis Copperkettle, Dorn's daughter; and Moss, the foundling. A cast that already has children gets no more.)
2. **How they look.** The collection has no child models, so a child is the adult mesh drawn at 0.6 scale: `pnpm assets` builds `Child_Knight`, `Child_Barbarian`, `Child_Mage`, `Child_Ranger`, `Child_Rogue`, `Child_Druid`, `Child_Engineer` and `Child_Farmer` from the adult files. Helmets, the bear hat, the mage's hat and the quiver are hidden; children carry nothing in their hands. They sit on chairs and benches like anyone else; the renderer lifts a small body onto the seat, since the sitting clip is made for a full-sized one.
3. **A child's day.** Breakfast, play, the midday meal, play or a stroll, play with the other children, supper, then home before dusk and asleep before the dead rise. Play happens on the green, at the docks and by the farm; children are drawn to each other and to their parents when they look for company. The model may send a child to a place, to one of the child's activities (eat, sleep, rest, play, pray, shop, stroll), to talk to someone, or let them carry on. A plan the model writes for a child is bent back to these: a block that names a job becomes play.
4. **Protected, not safe.** Children never fight and never answer the alarm; threatened, they run. The dead chase an adult before a child while any adult is in sight. The murderer does not hunt them, the drunk does not pick fights with them, and nobody calls them to a bucket chain. Nobody's grievance toward a child climbs past 30 (below "rival", far below a brawl), so no adult ever brawls with or attacks a child, and no child with anyone. Attraction never grows toward or from a child. A hostile word at a child is a telling-off. Children can still be hurt and can still die, and the village, parents first, grieves as it does for kin.
5. **Mischief.** Each child has a `mischief` number. At the turn of each day (midnight, with the rest of the daily reckoning) they roll it, and on a hit one piece of trouble is planned for the day and carried out when the chance comes:
   - **Pinching food**: from an adult's pack nearby, or from the tavern. The victim notices and holds it against the child a little.
   - **Throwing food**: an apple flies at an adult, who is startled and annoyed, and the child runs home.
   - **Hiding a tool**: an adult at work loses their tool for a while and the job runs long.
   - **Tall tales**: a made-up story about a villager, passed on in conversation, that sets adults a little against each other.
   - **Wandering off**: at dusk, toward the glade, the graveyard or the ruins instead of home. An adult who notices, or a parent, runs to fetch the child back; if nobody is awake outside, the dead may find them first.
   - **Playing with fire** (rare, and only for the naughtiest): a house catches in daylight and the village carries water as for any fire.
   Parents hear of every piece of trouble, so the model can have them scold.
6. **Growing up.** A child ages a year every three sim days. At sixteen they come of age: the same person, with their memories and bonds, now drawn as the adult of their class with its weapon, given the class's jobs and hobbies, an occupation ("young farmer"), a home with a free bed, and counted as an adult from then on. The chronicle records it and everyone remembers.
7. **Births.** Lovers who have been together eight days and are both alive may, at the turn of the day, with a one-in-six chance, have a child, if six days have passed since the last birth and fewer than six children are alive. The child is born aged five at the parents' home, asleep indoors, with a name and traits drawn from lists, the class of one parent, and kin bonds to both. The village is told as it is of a newcomer.

Population: the director's target counts adults only, so the founding's children do not hold newcomers back and a child who comes of age slows arrivals down. The status line's population counts everyone living.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `Character.child: Option<Childhood>` (`nextBirthdayDay`, `mischief`, `parents`, today's `plan`); `Character.partnerSinceTick`; `Director.lastBirthTick`; `StimulusKind::Family`; `IntentAction::Fetch`. All left out of a save when absent or zero, so older saves load and the fixtures round-trip. The adult class is `class_id` from birth, so no new class exists.
- **Data** (`server/crates/sim/src/data/children.rs`): the child looks (class, model, hidden parts), the names and traits, and every number above. `Blueprint.child` (`systems/founding.rs`) marks a child when a character is built; the test cast's `child` entries become one.
- **The system** (`server/crates/sim/src/systems/children.rs`): the founding's children (`found_children`, called when the founding concludes), birthdays, coming of age, births and mischief. `daily_children` runs at the day roll-over, `update_children` right after `update_quirks`. Nothing in it draws from the RNG unless a child is alive or a couple qualifies.
- **Play** is `ActivityId::Play`: slot-less like strolling, so no place changes, and `worldHash` and the golden fixtures stay as they were.
- **Protection** is spread thin on purpose: a cap in `relationships::adjust`, `open_to` in `util.rs`, and one-line exclusions in perception, combat, monsters, quirks and fire.
- **The brain** (`server/crates/brain`): a child's persona says they are a child and who raises them; the plan prompt says their days are for playing; the decide prompt offers a child's tools and activities. No react prompt is ever built for a child. The adult prompts do not change, except through the state.
- **The wire** (`wire.rs`): `RosterEntry.child`, `CharacterDetail.child`, and a `food` projectile. Additive, so the protocol version is unchanged.
- **The client**: a child marker in the roster, the inspector's "child · 9 · she/her" line, and an apple for the thrown food.

## Left out on purpose

- No babies or toddlers: a child is born old enough to run about.
- No schooling, chores or apprenticeships; a child's time is their own.
- A child's class comes from a parent, not from the child's interests.
- Children do not pair off, flirt or feud, and never carry weapons.
- Wanderers never arrive with children of their own.
