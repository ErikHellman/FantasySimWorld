# Animals

The valley has animals: a herd and hens at the windmill farm, ducks on the lake shore, birds about the green and over the fields, an owl in the glade, and a dog or a cat in about half the homes. Farmers tend the herd, villagers make a fuss of their pets and name them, and a villager setting off on a long walk rides a free horse. Animals run from the dead and the dead leave them be. The KayKit collection has no animals, so the models are made in Blender in the same style (`tools/blender/make_animals.py`).

## What happens

1. **Who lives here.** When a village's founding ends, or on the first tick of a world saved before there were animals, it gets 2 cows, 4 sheep, 2 goats and 3 horses in the meadow south of the windmill (hex 3,11), 2 pigs and 5 hens in the farmyard, 3 ducks on the lake shore, 4 robins and bluebirds about the green, the market and the tavern, 2 crows over the grain fields, and an owl in the glade. Horses come with names (Duke, Clover, …); the rest of the herd goes unnamed.
2. **Pets.** Each home with a grown-up in it has an even chance of a dog (three in five) or a cat, kept by one of the grown-ups there. A pet follows its keeper about while they are out (running to catch up beyond 9 paces), waits at the door of wherever they are while they are in, and sleeps there at night. Every two to four days a stray dog or cat comes in off the west road and adopts someone without a pet, while there are fewer pets than homes. A pet whose keeper dies goes to someone else in the house, and becomes a stray about the green if there is nobody.
3. **Names.** A new pet has no name. The sim emits `SimEvent::PetAdopted`, and the server asks its keeper, in their own voice, what to call it; the answer comes back as `Command::NameAnimal`. The sim takes one or two words of letters, capitalised, at most 16 UTF-16 units, and not a living villager's first name or another animal's name. After an hour of game time without one, the sim names the pet from its species' list. Either way the keeper remembers the name and the chronicle records it.
4. **A day in the meadow.** The herd wanders its haunt, stops to graze for 4 to 16 seconds, and wanders on; at night it beds down near the middle of the meadow. Hens and pigs keep closer, to the farmyard, and ducks to the shore. Birds peck about, hop a few paces, and now and then fly to another spot in their haunt; they sleep where they are at night. The owl keeps its perch, asleep by day and looking about by night.
5. **Tending.** `tend` is a farmer's job, between the plots and the woodpile. A farmer goes to one of the animals that give something (cows, goats and sheep, hens and ducks), stands beside it while it keeps still, and works for an hour or two. Tending yields food every hour, like the plots.
6. **Petting.** Now and then a keeper with their pet close by, or a child with any dog or cat close by, makes a fuss of it: walks up to it, kneels, and comes away a little less lonely (a ♥ over their head). A pet is fussed over at most once in three hours. The model may also choose to `pet` any animal near.
7. **Riding.** A grown villager setting out of doors on a walk of more than 70 paces, with a free horse within 25 paces, goes to the horse first if that is less than a third of the way. On it they travel at 1.9 times walking pace, the horse beneath them. They ride on from one errand to the next while the next is a long one too, and get down when it is short, when they go indoors, or when they stop to fight, flee or do anything else. The horse waits where they got down for six hours, and for them to come back for it; at nightfall, or when it tires of waiting, it walks home to the meadow.
8. **The dead.** An animal within 10 paces of one of the dead runs straight away from it for five seconds. The dead never go for animals: they only ever look for villagers.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`):
  - `SimState.animals` holds `Animal`s by id (`a1`, `a2`, …), with species, model, name, keeper, home, position, animation, path, flight, and riding and waiting.
  - `SimState.fauna` holds the animals' own bookkeeping: whether they have been given out, their own RNG, the next id and the next stray.
  - `Character.mount` and `Character.mounting` hold the horse a villager rides or is going to.
  - `IntentAction::Pet`, `ActivityId::Tend`, `AnimId::Ride` (the idle clip), `Command::NameAnimal` and `SimEvent::PetAdopted`.
  - Everything is left out of a save while empty, so older saves load and the fixtures round-trip.
- **Data** (`server/crates/sim/src/data/animals.rs`): each species' models, paces, haunt, wandering radius and names; the founding's numbers; and the riding pace.
- **The system** (`server/crates/sim/src/systems/animals.rs`, `update_animals` after `update_monsters`):
  - **Animals keep apart from the villagers' story.** They draw only from `fauna.rng`, never from the villagers' generator, take no part in `separate`, and nudge only themselves out of the way. A world goes on as it would without them, except where a villager deals with one.
  - **Each animal moves every fourth tick (`STRIDE`), on a tick of its own**, by four ticks' worth. A ridden horse moves with its rider every tick.
  - **They avoid blocked ground.** An animal never steps onto ground built over since its path was found, and steps off at once if something is built where it stands.
- **Villagers**:
  - `Tend` is run in `systems/intents.rs` like `Play`, with no slot. `tending_spot` and `hold_still` choose the animal and keep it still.
  - `run_pet` follows the pattern of picking up loot.
  - `travel` rides at `RIDE_PACE` while mounted.
  - `routine::choose` offers petting after company.
- **The brain** (`server/crates/brain`):
  - The world rule names animals beside people.
  - The header says where a villager's pet is and which horse they ride.
  - The decision prompt lists the animals within 22 paces (pets and horses by name, a neighbour's unnamed pet by its keeper's, the rest by kind) and offers `pet` only then.
  - `name_pet_prompt` asks for a name with the one tool `name_pet`, as a `name` request in the log; `tools/mock-llm.mjs` answers both.
- **The wire** (protocol 9):
  - Animals are the entity kind `animal`, after the villagers, the dead and the bodies, with the model in `look` and an empty name unless a pet or a horse.
  - `Dyn.mt` is the horse a villager rides, or a horse's rider.
  - `GET /api/animals/{id}` and `Detail::Animal` carry the card.
- **The client**:
  - `src/render/animals.ts` draws an animal from the `animals` bundle. Its parts are empties at the joints (so the bundle's mesh compression leaves the pivots alone), turned in code for walking, grazing, pecking, sleeping, sitting and flying. A bird in the air shows its `_flying` model at a height. The view eases each animal over the ticks between its updates.
  - A villager with `mt` sits on the horse's `seat` node, legs posed astride by `ActorView.ride`.
  - On `low` quality, small animals cast no shadow and only animals within 70 metres of the camera move their parts.

## Tests

`server/crates/sim/tests/animals.rs` covers:
- the founding of the animals;
- walkable ground over three days;
- running from the dead;
- following a keeper, naming, the fallback name, rehoming;
- a save round trip, sleep at night;
- the animals' generator staying apart;
- tending, petting, and a ride that beats walking, with the horse waiting and then going home at night.

`server/crates/brain/tests/animals.rs` covers the prompts, the `pet` tool and naming.

The stream test keeps a day's frames under 8 KB a second. Without the stride, animals alone pushed a day of the test village to 12.9 KB a second.

## Left out on purpose

- Breeding, ageing and deaths: the herd is the same herd for good, and the dead do not hunt livestock.
- Stables, coops and pens as buildings. The meadow and the farmyard are spots in the data, not places, so the static world, `worldHash` and the golden fixtures are unchanged.
- Wool as a material, and selling animals or their milk at the market.
- Animal sounds, and the owl ever flying.
- Children riding, and riding at a gallop into a fight.
- Pets for the dead. A pet goes to a housemate or becomes a stray; it never keeps a grave.
