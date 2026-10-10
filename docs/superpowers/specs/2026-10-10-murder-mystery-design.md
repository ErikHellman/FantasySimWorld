# The murder mystery

October 2026. Before this, a murderer who killed unseen was never found out: the chronicle said nobody saw who did it, and that was the end of it. Now the village tries to work it out. Viewers are told the truth from the start and watch the village get it right, or blame the wrong person. The village is still watched, not steered: nothing a viewer does reaches the sim.

## What happens

1. **A murderer arrives.** From the third day, a newcomer is now and then secretly a murderer: about one in eight, never a founder, and never while another murderer is in the valley. It is settled by a hash of the world's seed and the newcomer's name, not by the RNG, so a newcomer who is not a murderer arrives exactly as before. The model is not told; it described an ordinary traveller. The old behaviour of the quirk is unchanged: two days of settling in, then on a hunting day the murderer follows someone who is alone and strikes when nobody is looking.
2. **Viewers are let in on it.** The murderer's page carries a line in red ("A murderer, waiting for the chance to kill unseen", later naming whom they killed). Their status in the roster and on their page says whom they are stalking, the Auto camera can follow them (a stalking hotspot, scored 64, and a "Happening now" card), and their strike is a hotspot of its own (92). None of this reaches a villager's mind: their prompts never see the status line, the chronicle or the hotspots.
3. **Someone sees something.** While the murderer stalks, anyone within 15 metres may notice, about one chance in twelve a minute, settled by a hash of who, whom and when. The witness remembers it as merely odd. It matters only if the one followed is killed that day.
4. **A killing nobody saw opens a case.** The chronicle tells viewers at once who did it ("Only you know it: …"). The case starts with what the village already has: each witness who saw the killer trailing after the victim that day holds a sighting (worth 4), and the victim's closest friend knows of up to two people with a deep grudge against them (worth 2 each).
5. **The body is found.** The first villager to come within 7 metres finds it, or whoever is nearest after 50 minutes. They notice something beside it: more often than not (55%) it points at the killer, otherwise at someone with a grudge, or anyone. That trace is worth 3.
6. **Word gets round.** When two villagers talk (any conversation but a hostile one), each tells the other the weightiest thing they know that the other does not, unless it is about the listener. The listener trusts the suspect a little less.
7. **An accusation.** Once an hour, from eight hours after the killing, if no ceremony is pending: the village's suspicion of each grown villager is the weight of every clue against them times the number of people who know it. The most suspected, at 10 or more, is accused by whoever knows the weightiest clue against them, and a trial is called on the green at four in the afternoon (today, if called before one; otherwise tomorrow).
8. **The trial.** It reuses the ceremony machinery: the village gathers as for a wedding, the officiant opens, the accuser lays out the weightiest clue, the accused pleads, one voice from each side of the crowd speaks, and the officiant gives the verdict. The lines are scripted (`data/trials.rs`); the model writes none of them. Each villager present votes: the clues against the accused they know, plus the one aired at the trial, plus their grievance against the accused and minus their fondness, minus 6 for kin or a spouse; the killer adds 3, glad to see someone else take the blame. A belief of 4 or more is a guilty vote, and a majority convicts.
9. **The verdict.** The guilty are driven out: they walk to the west road and leave the valley for good, their bed freed for a newcomer, their name kept in `state.exiles`. Everyone present thinks less of them. If the village convicted the wrong person, the chronicle tells viewers so, and the real killer stays, free to kill again. The acquitted are cleared: what pointed at them is set aside, and they bear a grudge against their accuser. A case is tried at most twice.
10. **A cold case.** After a week with nobody convicted, the trail goes cold and the chronicle says the valley never learnt who did it.

## Where it lives

- `server/crates/sim/src/systems/cases.rs`: cases, clues, sightings, finding the body, gossip, accusations, votes, verdicts and exile.
- `server/crates/sim/src/systems/ceremonies.rs`: `schedule_trial` and `trial_step`, beside weddings and funerals; `Rite::Trial` in `state.rs`.
- `server/crates/sim/src/systems/arrivals.rs`: `comes_to_kill`, which makes a rare newcomer a murderer.
- `server/crates/server/src/hotspots.rs` and `digest.rs`: the stalking and striking hotspots and the `secret` line on a character's page. `CeremonyKind::Trial` on the wire (protocol 14).
- `src/ui/ceremony.ts` (the trial's banner), `src/ui/Inspector.tsx` (the secret), `src/ui/director.ts` (the card).
- Tests: `server/crates/sim/tests/cases.rs`.

## Left out on purpose

- **No model in the trial.** The speeches are scripted, so a trial costs nothing and happens the same with or without a model. The model still hears of it through memories, which can colour what villagers say afterwards.
- **No lies.** The murderer does not yet plant false clues or lie when asked; the trace beside the body pointing elsewhere is the only misdirection.
- **No viewer votes or hints.** Viewers know the truth and can do nothing with it, which is the point.
- **Founders are never murderers**, so a new world has a few days of peace, and the old cast's occultist leaning is still never given out.
