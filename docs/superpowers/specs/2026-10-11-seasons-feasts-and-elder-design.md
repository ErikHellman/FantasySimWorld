# Seasons, feasts and an elder

October 2026. The village had days but no year. Nothing came round again for a viewer to wait for, and nobody in the village stood above anyone else. Now the year has seasons, two feasts that the whole village keeps, and an elder it chooses by vote. Everything comes from what already drives the village: viewers only watch.

## What happens

1. **The year.** A year is four seasons of seven days, from spring on day 1, so day 29 begins year 2. The chronicle marks each new season ("Spring comes to the valley, and with it year 2."). The top bar shows the season next to the day, and the prompts tell the model what season it is.
2. **The harvest festival.** From the sixth day of autumn, the village keeps the harvest on the green at six in the evening. It is a ceremony like a wedding:
   - the elder (or, without one, whoever usually officiates) makes a speech;
   - the green raises a toast and cheers;
   - everyone goes on to a feast at the tavern.
   Everyone who came is in good spirits for a quarter of a day. If a wedding, funeral or trial is already arranged, the feast waits for a later day of autumn, or is missed that year.
3. **Midwinter.** From the fourth day of winter, the same ceremony takes place, with midwinter words. Every grudge between two villagers who were there drops by 8.
4. **The elder.** The village holds an election on the green at noon:
   - as soon as it is settled, from day 3, with at least four grown-ups;
   - again when its elder dies or is driven out;
   - every spring.
   Two candidates stand: the sitting elder, if there is one, and the grown-ups the village thinks best of (the sum of everyone's affinity plus half their trust). Each candidate speaks for themselves, in words the model writes when it is connected. Then the village votes:
   - candidates vote for themselves;
   - every other grown-up votes for whichever candidate they like and trust most;
   - a tie keeps the sitting elder, or the first named.
   The chronicle gives the count ("5 votes to 3"). Losing stings the loser's feelings toward the winner, and more for a sitting elder turned out. From then on the elder speaks the rite at every wedding, funeral and feast, and the elder's name is a button in the top bar that opens their page.
5. **Determinism.** Nothing here draws from the RNG. The calendar is the day number, and votes are what people already feel.

## Where it lives

- `server/crates/sim/src/systems/seasons.rs` has:
  - the calendar: `season_of`, `day_of_season` and `year_of`;
  - `festival_due`, `election_due` and `candidates`;
  - `tally`, `winner` and `seat_elder`, for the vote;
  - `keep_festival`;
  - `daily_seasons`, for the chronicle line.
- `server/crates/sim/src/systems/ceremonies.rs` arranges and runs the new rites: `Rite::Festival` and `Rite::Election`, `festival_step` and `election_step`. `officiant_for` prefers the elder. The words are in `server/crates/sim/src/data/seasons.rs`.
- `SimState.village` (`Village` in `state.rs`) holds the elder and the year of the last election, harvest and midwinter. It is left out of saves while it is empty, so older saves load unchanged.
- The model writes the feast speech (`festival_prompt`) and each candidate's speech (`campaign_prompt`) in `server/crates/brain/src/prompts.rs`. Every prompt's header now names the season and the elder.
- On the wire (protocol 15):
  - `Status` gains `season`, `year` and `elder`;
  - `CeremonyKind` gains `festival` and `election`, and `Ceremony.feast` says which feast;
  - `CharacterDetail` gains `elderSince`.
- The client shows the season beside the day, the elder pill in the top bar, a banner for each new rite, and "Elder of the village since day N" on the elder's page.
- Tests: `server/crates/sim/tests/seasons.rs`.

## Left out on purpose

- **The weather does not follow the seasons.** Snow in winter, and longer days in summer, would mean touching the weather system and the lighting. That is for later.
- **The elder has no powers.** The elder speaks at ceremonies and is respected, but does not set laws or settle quarrels. Powers would mean the elder steering the village, and that should come from the model's choices, not from new rules.
- **No campaigning between elections.** Candidates are told they stand and to win people over, and the model may act on it. Nothing new tracks a campaign.
- **Two candidates only.** A third splits the vote in ways the viewers cannot follow from the chronicle.
