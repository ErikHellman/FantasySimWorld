# Weddings and funerals

Two moments that bring the whole village to one place. A couple who have lasted are married on the green before everyone, and move in together. The dead are buried the next morning with the whole village at the graveside. Both are told to the language model, which writes the vows and the eulogy ahead of time; scripted lines stand in when no answer comes.

## What happens

### Weddings

1. **A couple is betrothed.** At midnight, the first couple who have been partners for two days, like each other well (affinity 40 or more both ways, grievance under 20), are not kin and are not already married, is betrothed, if no other ceremony is pending. The chronicle says who will marry whom, on the green at two o'clock the next day, and who will speak the rite. The couple remember it (importance 8), everyone else hears of it.
2. **The officiant.** A cleric, failing that a paladin, failing that a druid; otherwise the eldest villager. Never one of the couple. With the founding cast it is Rowan the druid.
3. **The speeches are written.** The sim tells the brain the wedding is arranged. When the model has room, each half of the couple is asked for the vow they will speak (two or three short sentences, in their own voice), with what they remember of the other. The answers are cleaned, cut to 300 characters and held on the ceremony. A wedding without them uses scripted vows.
4. **The village gathers.** Forty minutes before the hour, everyone who is not fighting, fleeing, carrying water, beaten or badly hurt is called to the green, asleep or not: they drop what they were doing and walk to a spot of their own around the couple. Nothing the model asks for takes anyone away except a fight or a flight. The couple stand facing each other south of the well, the officiant before them, the guests in an arc.
5. **The rite.** When the couple are in place (or fifteen minutes past the hour), the officiant opens, each of the couple speaks their vow, and the officiant pronounces them wed. The couple's hearts show; everyone cheers, and two guests call out good wishes.
6. **Married.** The two are spouses. Their relationship is labelled "spouse" (the roster still shows them as partners), and they part only when things are far worse than would end a courtship (affinity below -10 or grievance of 80 or more). They move in together: into whichever of their homes stands and has fewer other people in it, so a bed is freed for a newcomer. The couple remember the day at importance 10, the guests at 6; guests warm to each other a little; anyone who was sweet on either of them takes it hard (grievance and a broken heart toward the other spouse). Hunger and company are satisfied by the feast.
7. **The feast.** For an hour the village eats and drinks at the tavern, then the day goes on.
8. **Put off, or called off.** A wedding is put off a day when a house is burning, when one of the dead stands within 18 units of the green (one lying low at the ruins by day does not count), or when a half of the couple is badly hurt or fighting. After three such days it is called off. The death of either half calls it off at once; a death in the village while the wedding is only announced gives way to the funeral, and the couple are betrothed again at the next midnight.

### Funerals

1. **The dead wait for morning.** When a villager dies and has a grave, they are added to those awaiting burial, and a funeral is arranged at the graveyard at nine the next morning, or the same morning for a death before seven. A villager lost to necromancy is buried only once the necromancer they became is destroyed and a grave is dug. One funeral buries everyone waiting.
2. **The officiant** is chosen as for a wedding, and asked for a eulogy: who died, how, and who grieves most, with their memories of the dead.
3. **The village gathers** at the graveside as it does for a wedding, forty minutes ahead. Funerals are held by day only; a funeral is put off when a house burns or when the dead are abroad near the graves.
4. **The rite.** The officiant opens and speaks the eulogy; then the closest mourners (a spouse, kin, close friends, at most three) each say a line of grief, and the spouse and kin sit by the grave; the officiant closes. Everyone remembers it: a spouse and kin at importance 10, with a grief they will carry; close friends at 8; others at 5. Mourners trust the people who stood with them a little more.
5. **The wake** follows at the tavern for an hour.

## Where it lives

- **State** (`server/crates/sim/src/state.rs`): `Character.spouse_id` and `Character.partner_since_tick`; `RelationLabel::Spouse`; `Director.awaiting_funeral`; `SimState.ceremony: Option<Ceremony>` (the rite, the place, the hour, the phase and step, the officiant, the held speeches, who was called, how often it was put off); `IntentAction::Attend`. All are left out of a save when empty or absent, so older saves and the fixtures load unchanged.
- **The system** (`server/crates/sim/src/systems/ceremonies.rs`): betrothal at the midnight roll-over, scheduling from `kill_character` and `bury_the_fallen`, the muster (after `fire::muster`), the spots, the rite's steps, the conclusion and the feast. Its constants are at the top. `update_ceremonies` runs right after `update_fire`. Nothing here draws from the RNG until a rite is under way, so a world with no couple and no death runs exactly as before.
- **Spouses** (`systems/relationships.rs`): `marry`, the spouse label and the harder parting; `kill_character` and `fall_to_necromancy` widow the living.
- **Scripted lines** (`data/ceremonies.rs`): openings, vows, closings, cheers, eulogies and lines of grief, picked with the sim's dice.
- **The brain** (`server/crates/brain`): `RequestKind::Ceremony`, the `speech` tool, `vows_prompt` and `eulogy_prompt`, asked when `SimEvent::CeremonyScheduled` arrives and answered with `Command::CeremonySpeech`, which the sim keeps only while the ceremony still stands and the speaker still speaks at it. The prompt header tells everyone about today's or tomorrow's ceremony, and a spouse is named as "your wife", "your husband" or "your spouse". These are deliberate changes to the prompt fixture.
- **The wire** (`wire.rs`, protocol version 6): `Snapshot.ceremony` and the `ceremony` event, found by comparing state as houses are, with a final `over` phase; `RelationLabel::Spouse`; `CharacterDetail.spouse`; `RequestKind::Ceremony`.
- **The client**: a banner for the announcement, the gathering and the feast; the inspector's spouse badge and "Married to"; `src/render/ceremony.ts` dresses the green with an arch, banners and flowers for a wedding and the graveside with a coffin and candles for a funeral; the music turns to celebration for a wedding and to grief for a funeral, and the church bells ring at the pronouncement.

## Left out on purpose

- No viewer shop offers, and no wedding gifts.
- No children, and no divorce ceremony: a parting is as quiet as it is today.
- A second ceremony is never arranged while one is pending; the dead wait, and a couple are betrothed again later.
- The model is asked once, when the ceremony is arranged; if the officiant dies before the day, the scripted lines are used.
- No new sounds: the alarm's church bells serve the wedding, and a funeral has none. A slower bell can be cut from the same source later with `pnpm audio`.
