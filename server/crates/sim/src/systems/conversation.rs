//! Conversations: hailing, lines spoken in turn, waiting for the brain to
//! write more, and what everyone takes away from them.

use fsw_core::jsmath::utf16_truncate;
use fsw_core::math2d::{dist, facing_to};
use fsw_core::time::{minutes_to_ticks, seconds_to_ticks};

use crate::data::anims::AnimId;
use crate::data::classes::WALK_SPEED;
use crate::data::smalltalk::{generic_continuation, local_script};
use crate::events::{Emote, SimEvent};
use crate::state::{
    ActPhase, ChronicleKind, Conversation, ConversationLine, ConvoAuthor, ConvoPhase, Fallback, Feeling, Id,
    IntentAction, Map, MemoryKind, Quirk, StimulusKind, TalkTone,
};
use crate::systems::charm;
use crate::systems::movement::{advance, face, route_to};
use crate::systems::quirks::{pass_on_rumour, pick_rumour};
use crate::systems::relationships::{RelationDelta, adjust, apply_conversation, become_lovers};
use crate::util::{Ctx, bump_epoch, first_name, is_fighting, nearby_characters, place_name_at, rel, set_anim, their};

pub const MAX_LINES: usize = 8;
const TALK_DISTANCE: f64 = 2.2;
const GESTURES: [AnimId; 3] = [AnimId::Interact, AnimId::IdleAlt, AnimId::Wave];

fn hail_timeout() -> i64 {
    seconds_to_ticks(14.0)
}

/// Starts a conversation: `a` has reached `b` and hails them.
pub fn start_conversation(ctx: &mut Ctx, a: &str, b: &str, tone: TalkTone, opener: &str) -> Id {
    let rumour = if tone == TalkTone::Friendly { pick_rumour(ctx, a, b) } else { None };
    let script = if opener.is_empty() {
        local_script(ctx.state, a, b, tone, rumour.as_ref())
    } else {
        generic_continuation(ctx.state, a, b, tone, rumour.as_ref())
    };
    // With no opening line given and a model to wait for, the whole conversation is left for it to
    // write; the locally written one (opener included) is kept as the fallback.
    let await_opener = opener.is_empty() && ctx.state.tuning.script_wait_ticks > 0;
    let local_opener = ConversationLine {
        speaker_id: a.to_owned(),
        text: if opener.is_empty() { script.opener.clone() } else { opener.to_owned() },
    };
    let id = ctx.state.new_id("v");
    let mut feelings = Map::new();
    feelings.insert(a.to_owned(), script.feelings[0]);
    feelings.insert(b.to_owned(), script.feelings[1]);
    let convo = Conversation {
        id: id.clone(),
        a: a.to_owned(),
        b: b.to_owned(),
        tone,
        phase: ConvoPhase::Hail,
        lines: if await_opener { Vec::new() } else { vec![local_opener.clone()] },
        index: 0,
        line_end_tick: 0,
        started_tick: ctx.tick(),
        await_until_tick: 0,
        closed: false,
        author: ConvoAuthor::Local,
        topic: script.topic.clone(),
        feelings: Map::new(),
        fallback: Fallback {
            lines: if await_opener { [vec![local_opener], script.lines].concat() } else { script.lines },
            topic: script.topic,
            feelings,
        },
        rumour,
    };
    ctx.state.conversations.insert(id.clone(), convo);
    for c in [a, b] {
        let c = ctx.c(c);
        c.convo_id = Some(id.clone());
        bump_epoch(c);
    }
    // Being hailed is a reflex: stop, turn, wave. No decision needed.
    let bc = ctx.c(b);
    if !bc.anim.is_seated() {
        bc.act.path.clear();
        set_anim(bc, if tone == TalkTone::Hostile { AnimId::Idle } else { AnimId::Wave }, true);
    }
    ctx.emit(SimEvent::ConversationStarted { convo_id: id.clone(), a: a.into(), b: b.into(), tone });
    id
}

fn speak(ctx: &mut Ctx, convo_id: &str) {
    let convo = &ctx.state.conversations[convo_id];
    let line = convo.lines[convo.index as usize].clone();
    let listener = if line.speaker_id == convo.a { convo.b.clone() } else { convo.a.clone() };
    let (hostile, index) = (convo.tone == TalkTone::Hostile, convo.index);
    if !ctx.alive(&line.speaker_id) {
        return;
    }
    let tick = ctx.tick();
    let duration = ctx.say(&line.speaker_id, line.text, Some(&listener), hostile);
    ctx.state.conversations[convo_id].line_end_tick = tick + duration + seconds_to_ticks(0.35);
    let speaker = ctx.c(&line.speaker_id);
    if !speaker.anim.is_seated() {
        set_anim(speaker, GESTURES[index as usize % GESTURES.len()], true);
    }
}

fn use_fallback(convo: &mut Conversation) {
    let room = MAX_LINES.saturating_sub(convo.lines.len());
    if convo.lines.is_empty() {
        // Nothing was said yet: the local conversation is used whole, opener and all.
        convo.lines.extend(convo.fallback.lines.iter().take(room).cloned());
        convo.closed = true;
        convo.topic = convo.fallback.topic.clone();
        return;
    }
    // The local lines were written to follow the opener; keep speakers alternating whatever came since.
    let last = convo.lines.last().map_or(convo.a.clone(), |line| line.speaker_id.clone());
    let first = if last == convo.a { convo.b.clone() } else { convo.a.clone() };
    let available = convo.fallback.lines.len();
    let count = if convo.lines.len() > 1 { available.min(2) } else { available };
    let lines: Vec<ConversationLine> = convo.fallback.lines[available - count..]
        .iter()
        .enumerate()
        .map(|(index, line)| ConversationLine {
            speaker_id: if index % 2 == 0 { first.clone() } else { last.clone() },
            text: line.text.clone(),
        })
        .collect();
    convo.lines.extend(lines.into_iter().take(room));
    convo.closed = true;
    if convo.topic.is_empty() {
        convo.topic = convo.fallback.topic.clone();
    }
}

/// Lines written by the LLM (a whole script, or one reply at a time).
pub fn add_lines(
    ctx: &mut Ctx,
    convo_id: &str,
    lines: &[ConversationLine],
    closed: bool,
    topic: Option<&str>,
    feelings: Option<&Map<Feeling>>,
) {
    let Some(convo) = ctx.state.conversations.get_mut(convo_id) else { return };
    if convo.closed {
        return;
    }
    let room = MAX_LINES.saturating_sub(convo.lines.len());
    let valid: Vec<ConversationLine> = lines
        .iter()
        .filter(|line| (line.speaker_id == convo.a || line.speaker_id == convo.b) && !line.text.trim().is_empty())
        .take(room)
        .map(|line| ConversationLine {
            speaker_id: line.speaker_id.clone(),
            text: utf16_truncate(line.text.trim(), 240).to_owned(),
        })
        .collect();
    convo.lines.extend(valid);
    convo.author = ConvoAuthor::Llm;
    if let Some(topic) = topic.filter(|t| !t.is_empty()) {
        convo.topic = utf16_truncate(topic, 80).to_owned();
    }
    if let Some(feelings) = feelings {
        for (id, feeling) in feelings {
            convo.feelings.insert(id.clone(), *feeling);
        }
    }
    if closed || convo.lines.len() >= MAX_LINES {
        convo.closed = true;
    }
}

pub fn force_local(ctx: &mut Ctx, convo_id: &str) {
    if let Some(convo) = ctx.state.conversations.get_mut(convo_id)
        && !convo.closed
    {
        use_fallback(convo);
    }
}

/// Frees both speakers and drops the conversation; returns it with whichever of the two are still alive.
fn release(ctx: &mut Ctx, convo_id: &str) -> (Conversation, Option<Id>, Option<Id>) {
    let convo = ctx.state.conversations.shift_remove(convo_id).expect("the conversation exists");
    let tick = ctx.tick();
    let mut alive = [None, None];
    for (slot, id) in [&convo.a, &convo.b].into_iter().enumerate() {
        if let Some(c) = ctx.state.characters.get_mut(id) {
            if c.convo_id.as_deref() == Some(convo_id) {
                c.convo_id = None;
                c.next_social_tick = tick + minutes_to_ticks(35.0 + (1.0 - c.profile.sociability) * 50.0);
            }
            alive[slot] = Some(id.clone());
        }
    }
    let [a, b] = alive;
    (convo, a, b)
}

/// Ends a conversation that was cut short (someone was attacked, died or walked into a fight).
pub fn abort_conversation(ctx: &mut Ctx, convo_id: &str) {
    if !ctx.state.conversations.contains_key(convo_id) {
        return;
    }
    let (convo, a, b) = release(ctx, convo_id);
    if let Some(a) = &a
        && matches!(ctx.cr(a).intent.as_ref().map(|i| &i.action), Some(IntentAction::Talk { .. }))
    {
        let other = b.as_deref().map_or("them".to_owned(), |b| first_name(ctx.cr(b)).to_owned());
        let c = ctx.c(a);
        c.outcome = format!("Your talk with {other} was cut short.");
        c.act.phase = ActPhase::Done;
    }
    ctx.emit(SimEvent::ConversationEnded { convo_id: convo_id.into(), a: convo.a, b: convo.b, feelings: Map::new() });
}

/// How a hard word between a grown-up and a child reads: as a telling-off, not a quarrel.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Scolding {
    None,
    Gave,
    Got,
}

fn summary_of(convo: &Conversation, self_id: &str, other_name: &str, scolding: Scolding) -> String {
    let said: Vec<&str> =
        convo.lines.iter().filter(|line| line.speaker_id != self_id).map(|line| line.text.as_str()).collect();
    let heard =
        said.last().map_or(String::new(), |last| format!(" {other_name} said: \"{}\"", utf16_truncate(last, 110)));
    let topic = if convo.topic.is_empty() { "this and that" } else { &convo.topic };
    match scolding {
        Scolding::Gave => return format!("Told {other_name} off about {topic}.{heard}"),
        Scolding::Got => return format!("{other_name} told me off about {topic}.{heard}"),
        Scolding::None => {}
    }
    let how = match convo.tone {
        TalkTone::Hostile => "Had words with",
        TalkTone::Romantic => "Had a tender talk with",
        TalkTone::Apology => "Talked about making amends with",
        TalkTone::Friendly => "Talked with",
    };
    format!("{how} {other_name} about {topic}.{heard}")
}

fn feeling_note(feeling: Feeling) -> &'static str {
    match feeling {
        Feeling::MuchWorse => "I came away furious.",
        Feeling::Worse => "It left a sour taste.",
        Feeling::Same => "",
        Feeling::Better => "I feel better for it.",
        Feeling::MuchBetter => "It lifted my heart.",
    }
}

fn finish(ctx: &mut Ctx, convo_id: &str) {
    let (convo, a, b) = release(ctx, convo_id);
    let (Some(a), Some(b)) = (a, b) else { return };
    let mut feelings = convo.fallback.feelings.clone();
    for (id, feeling) in &convo.feelings {
        feelings.insert(id.clone(), *feeling);
    }
    let feel_a = feelings.get(&a).copied().unwrap_or(Feeling::Same);
    let feel_b = feelings.get(&b).copied().unwrap_or(Feeling::Same);

    apply_conversation(ctx, &a, &b, feel_a, convo.tone, true);
    apply_conversation(ctx, &b, &a, feel_b, convo.tone, false);
    for id in [&a, &b] {
        let c = ctx.c(id);
        c.needs.social = (c.needs.social + 15.0).min(100.0);
    }

    let (a_name, b_name) = (first_name(ctx.cr(&a)).to_owned(), first_name(ctx.cr(&b)).to_owned());
    // A grown-up's hard word at a child is a telling-off, whichever of them started it.
    let (a_child, b_child) = (ctx.cr(&a).is_child(), ctx.cr(&b).is_child());
    let told_off = convo.tone == TalkTone::Hostile && a_child != b_child;
    let scolding = |me_child: bool| match (told_off, me_child) {
        (false, _) => Scolding::None,
        (true, true) => Scolding::Got,
        (true, false) => Scolding::Gave,
    };
    let weight = if convo.tone == TalkTone::Friendly { 3.0 } else { 6.0 };
    let strong = |f: Feeling| if matches!(f, Feeling::MuchWorse | Feeling::MuchBetter) { 2.0 } else { 0.0 };
    let memory = |self_id: &str, other: &str, f: Feeling, me_child: bool| {
        format!("{} {}", summary_of(&convo, self_id, other, scolding(me_child)), feeling_note(f)).trim().to_owned()
    };
    ctx.remember(&a, memory(&a, &b_name, feel_a, a_child), weight + strong(feel_a), MemoryKind::Talk, vec![b.clone()]);
    ctx.remember(&b, memory(&b, &a_name, feel_b, b_child), weight + strong(feel_b), MemoryKind::Talk, vec![a.clone()]);

    let both = vec![a.clone(), b.clone()];
    match convo.tone {
        TalkTone::Hostile => {
            let heated = feel_a == Feeling::MuchWorse || feel_b == Feeling::MuchWorse;
            ctx.emit(SimEvent::Emote { id: a.clone(), emote: Emote::Anger });
            if matches!(feel_b, Feeling::Worse | Feeling::MuchWorse) {
                ctx.emit(SimEvent::Emote { id: b.clone(), emote: Emote::Anger });
            }
            let a_pos = ctx.cr(&a).pos;
            let text = if told_off {
                let (grown, child) = if a_child { (&b_name, &a_name) } else { (&a_name, &b_name) };
                format!("{grown} told {child} off near {}.", place_name_at(a_pos))
            } else if heated {
                format!("{a_name} and {b_name} had a furious row near {}.", place_name_at(a_pos))
            } else if feel_a == Feeling::Better && feel_b == Feeling::Better {
                format!("{a_name} confronted {b_name}, and they cleared the air.")
            } else {
                format!("{a_name} confronted {b_name} near {}.", place_name_at(a_pos))
            };
            ctx.chronicle(ChronicleKind::Conflict, text, both.clone());
            for witness in nearby_characters(ctx.state, a_pos, 11.0, None) {
                if witness == a || witness == b || ctx.cr(&witness).asleep {
                    continue;
                }
                ctx.remember(
                    &witness,
                    format!("Overheard {a_name} and {b_name} quarrelling about {}.", convo.topic),
                    4.0,
                    MemoryKind::Event,
                    both.clone(),
                );
                adjust(ctx, &witness, &a, RelationDelta { affinity: -2.0, ..Default::default() });
            }
            charm::revel(ctx, &a, &b, false);
        }
        TalkTone::Romantic => {
            let accepted = feel_a.is_good() && feel_b.is_good();
            let ab = rel(ctx.c(&a), &b).clone();
            let ba = rel(ctx.c(&b), &a).clone();
            let mutual = ab.attraction >= 45.0 && ba.attraction >= 45.0;
            let free = ctx.cr(&a).partner_id.is_none() && ctx.cr(&b).partner_id.is_none();
            let (a_allure, b_allure) = (charm::allure(ctx.state, &a), charm::allure(ctx.state, &b));
            if ctx.cr(&b).has(Quirk::Chaste) {
                // Courting someone who has sworn off love: no answer, and no understanding either.
                ctx.emit(SimEvent::Emote { id: b.clone(), emote: Emote::Thinking });
                ctx.chronicle(
                    ChronicleKind::Romance,
                    format!("{a_name} tried to court {b_name}, who did not seem to understand a word of it."),
                    both.clone(),
                );
            } else if accepted && mutual && free && !ab.kin {
                become_lovers(ctx, &a, &b);
            } else if accepted {
                ctx.emit(SimEvent::Emote { id: a.clone(), emote: Emote::Heart });
                let text = if a_allure > 0.0 || b_allure > 0.0 {
                    format!("{a_name} was seen flirting with {b_name}.")
                } else {
                    format!("{a_name} and {b_name} were seen talking very close together.")
                };
                ctx.chronicle(ChronicleKind::Romance, text, both.clone());
            } else if b_allure >= charm::CHARMER {
                // A charmer turns nobody away for good: she teases, and he stays hooked.
                ctx.emit(SimEvent::Emote { id: b.clone(), emote: Emote::Laugh });
                ctx.chronicle(
                    ChronicleKind::Romance,
                    format!("{b_name} teased {a_name} and promised nothing."),
                    both.clone(),
                );
            } else if a_allure >= charm::CHARMER {
                // Her heart was never in it; only her pride is stung.
                adjust(ctx, &a, &b, RelationDelta { grievance: 3.0, ..Default::default() });
                ctx.chronicle(
                    ChronicleKind::Romance,
                    format!("{b_name} would not rise to {a_name}'s flirting."),
                    both.clone(),
                );
            } else {
                ctx.emit(SimEvent::Emote { id: a.clone(), emote: Emote::BrokenHeart });
                adjust(ctx, &a, &b, RelationDelta { attraction: -8.0, grievance: 5.0, ..Default::default() });
                let pronoun = their(ctx.cr(&a));
                ctx.chronicle(
                    ChronicleKind::Romance,
                    format!("{a_name} opened {pronoun} heart to {b_name}, and was gently turned away."),
                    both.clone(),
                );
            }
            // A partner who is close enough to see takes flirting badly.
            for (flirt, other) in [(&a, &b), (&b, &a)] {
                let f = ctx.cr(flirt);
                let Some(partner) = f.partner_id.clone().filter(|p| ctx.alive(p)) else { continue };
                let p = ctx.cr(&partner);
                if &partner == other || p.inside.is_some() || dist(p.pos, f.pos) >= 14.0 {
                    continue;
                }
                let (flirt_name, other_name) = (first_name(f).to_owned(), first_name(ctx.cr(other)).to_owned());
                adjust(ctx, &partner, other, RelationDelta { grievance: 20.0, affinity: -15.0, ..Default::default() });
                adjust(
                    ctx,
                    &partner,
                    flirt,
                    RelationDelta { grievance: 14.0, affinity: -12.0, trust: -15.0, ..Default::default() },
                );
                ctx.remember(
                    &partner,
                    format!("Saw {flirt_name} making eyes at {other_name}. It cut deep."),
                    8.0,
                    MemoryKind::Event,
                    vec![flirt.clone(), other.clone()],
                );
                ctx.stimulate(
                    &partner,
                    StimulusKind::Romance,
                    format!("You saw your partner {flirt_name} in a tender conversation with {other_name}."),
                    2,
                    Some(other),
                );
                ctx.emit(SimEvent::Emote { id: partner.clone(), emote: Emote::Anger });
            }
            charm::stir_jealousy(ctx, &a, &b);
            if accepted {
                charm::work_charm(ctx, &a, &b);
                charm::work_charm(ctx, &b, &a);
            }
        }
        TalkTone::Apology => {
            let text = if feel_b.is_good() {
                format!("{a_name} apologised to {b_name}, and was forgiven.")
            } else {
                format!("{a_name} apologised to {b_name}, who was not ready to hear it.")
            };
            ctx.chronicle(ChronicleKind::Social, text, both.clone());
        }
        TalkTone::Friendly => {
            if feel_a == Feeling::MuchBetter && feel_b == Feeling::MuchBetter {
                ctx.emit(SimEvent::Emote { id: a.clone(), emote: Emote::Laugh });
                ctx.emit(SimEvent::Emote { id: b.clone(), emote: Emote::Laugh });
            }
            if let Some(rumour) = &convo.rumour {
                pass_on_rumour(ctx, &a, &b, rumour);
            }
        }
    }

    let topic = if convo.topic.is_empty() { "this and that".to_owned() } else { convo.topic.clone() };
    if matches!(ctx.cr(&a).intent.as_ref().map(|i| &i.action), Some(IntentAction::Talk { .. })) {
        let c = ctx.c(&a);
        c.outcome = format!("You talked with {b_name} about {topic}.");
        c.act.phase = ActPhase::Done;
    }
    ctx.stimulate(&b, StimulusKind::Talked, format!("{a_name} came to talk with you about {topic}."), 0, Some(&a));
    let mut ended = Map::new();
    ended.insert(a.clone(), feel_a);
    ended.insert(b.clone(), feel_b);
    ctx.emit(SimEvent::ConversationEnded { convo_id: convo.id, a, b, feelings: ended });
}

pub fn update_conversations(ctx: &mut Ctx) {
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let ids: Vec<Id> = ctx.state.conversations.keys().cloned().collect();
    for id in ids {
        let Some(convo) = ctx.state.conversations.get(&id) else { continue };
        let (a, b) = (convo.a.clone(), convo.b.clone());
        let broken = match (ctx.state.characters.get(&a), ctx.state.characters.get(&b)) {
            (Some(ac), Some(bc)) => is_fighting(ac) || is_fighting(bc) || ac.asleep || bc.asleep,
            _ => true,
        };
        if broken {
            abort_conversation(ctx, &id);
            continue;
        }

        let a_pos = ctx.cr(&a).pos;
        let bc = ctx.c(&b);
        if !bc.anim.is_seated() {
            face(bc, a_pos);
        }

        let phase = ctx.state.conversations[&id].phase;
        if phase == ConvoPhase::Hail {
            let b_pos = ctx.cr(&b).pos;
            let d = dist(a_pos, b_pos);
            let timed_out = tick - ctx.state.conversations[&id].started_tick > hail_timeout();
            if d > TALK_DISTANCE && !timed_out {
                let ac = ctx.c(&a);
                if ac.act.path.is_empty() || tick >= ac.act.repath_tick {
                    ac.act.path = route_to(&nav, ac.pos, b_pos).unwrap_or_default();
                    ac.act.repath_tick = tick + seconds_to_ticks(1.0);
                }
                // Stop a comfortable distance short rather than standing on their toes.
                if d > TALK_DISTANCE + 0.3 {
                    let mut path = std::mem::take(&mut ac.act.path);
                    let speed = WALK_SPEED * ac.stride();
                    advance(ac, &mut path, speed);
                    ac.act.path = path;
                    set_anim(ac, AnimId::Walk, false);
                }
                continue;
            }
            let ac = ctx.c(&a);
            ac.act.path.clear();
            ac.facing = facing_to(a_pos, b_pos);
            let bc = ctx.c(&b);
            if !bc.anim.is_seated() {
                bc.facing = facing_to(b_pos, a_pos);
            }
            let wait = ctx.state.tuning.script_wait_ticks;
            let convo = ctx.state.conversations.get_mut(&id).expect("still there");
            if wait == 0 && !convo.closed {
                use_fallback(convo);
            }
            if convo.lines.is_empty() {
                // Face to face, but the opening line has not been written yet.
                convo.phase = ConvoPhase::Await;
                convo.index = -1;
                convo.await_until_tick = tick + wait;
                ctx.emit(SimEvent::ConversationWaiting { convo_id: id.clone() });
                ctx.emit(SimEvent::Emote { id: a.clone(), emote: Emote::Thinking });
                continue;
            }
            convo.phase = ConvoPhase::Play;
            convo.index = 0;
            speak(ctx, &id);
            continue;
        }

        let b_pos = ctx.cr(&b).pos;
        let ac = ctx.c(&a);
        if !ac.anim.is_seated() {
            face(ac, b_pos);
        }

        let convo = &ctx.state.conversations[&id];
        let (index, len, closed, line_end) = (convo.index, convo.lines.len() as i64, convo.closed, convo.line_end_tick);
        if phase == ConvoPhase::Play {
            if tick < line_end {
                // Return to a neutral stance once the gesture has played.
                for c in [&a, &b] {
                    let c = ctx.c(c);
                    if !c.anim.is_seated() && !c.anim.loops() && tick - line_end > -seconds_to_ticks(0.4) {
                        set_anim(c, AnimId::Idle, false);
                    }
                }
                continue;
            }
            if index + 1 < len {
                ctx.state.conversations[&id].index += 1;
                speak(ctx, &id);
            } else if closed {
                finish(ctx, &id);
            } else {
                let wait = ctx.state.tuning.script_wait_ticks;
                let convo = &mut ctx.state.conversations[&id];
                convo.phase = ConvoPhase::Await;
                convo.await_until_tick = tick + wait;
                let next = if convo.lines.get(index as usize).is_some_and(|line| line.speaker_id == convo.a) {
                    b.clone()
                } else {
                    a.clone()
                };
                for c in [&a, &b] {
                    let c = ctx.c(c);
                    if !c.anim.is_seated() {
                        set_anim(c, AnimId::Idle, false);
                    }
                }
                ctx.emit(SimEvent::ConversationWaiting { convo_id: id.clone() });
                ctx.emit(SimEvent::Emote { id: next, emote: Emote::Thinking });
            }
            continue;
        }

        // Awaiting more lines.
        if index + 1 < len {
            let convo = &mut ctx.state.conversations[&id];
            convo.phase = ConvoPhase::Play;
            convo.index += 1;
            speak(ctx, &id);
        } else if closed {
            finish(ctx, &id);
        } else if tick >= ctx.state.conversations[&id].await_until_tick {
            let convo = &mut ctx.state.conversations[&id];
            use_fallback(convo);
            if convo.index + 1 >= convo.lines.len() as i64 {
                finish(ctx, &id);
            }
        }
    }
}

/// Whether two characters have spoken too recently to start again (180 minutes in the TypeScript default).
pub fn talked_recently(ctx: &mut Ctx, a: &str, b: &str, minutes: f64) -> bool {
    let tick = ctx.tick();
    tick - rel(ctx.c(a), b).last_talk_tick < minutes_to_ticks(minutes)
}

/// Random chance scaled by how talkative the sim is currently allowed to be.
pub fn chatter_roll(ctx: &mut Ctx, id: &str) -> bool {
    let c = ctx.cr(id);
    let need = (100.0 - c.needs.social) / 100.0;
    let p = ctx.state.tuning.chatter * (0.25 + c.profile.sociability * 0.5 + need * 0.4);
    ctx.chance(p)
}
