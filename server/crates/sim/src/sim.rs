//! The simulation. `step()` advances one tick (1/20 s of real time at 1x) and
//! returns that tick's one-shot events. Commands are applied at tick boundaries.

use fsw_core::jsmath::utf16_truncate;
use fsw_core::time::game_time;
use fsw_world::places::{find_billboard, find_place};

use crate::commands::Command;
use crate::data::anims::AnimId;
use crate::events::SimEvent;
use crate::state::{
    ActivityId, BoardNotice, BoardReaction, ChronicleKind, FightMode, HeraldIssue, Intent, IntentAction, MemoryKind,
    PlanBlock, PlanSource, SimState, WorldJob,
};
use crate::systems::ambitions::update_ambitions;
use crate::systems::animals::{self, update_animals};
use crate::systems::billboards::{
    TEXT_CAP, board_index, clean_text, day_tone, has_read, react, reading, shown, wants_words,
};
use crate::systems::blocking::update_blocking;
use crate::systems::cases::update_cases;
use crate::systems::ceremonies::{self, attending, update_ceremonies};
use crate::systems::children::{daily_children, fetching, update_children};
use crate::systems::combat::{resolve_mercy, start_fight, start_flee};
use crate::systems::conversation::{add_lines, force_local, update_conversations};
use crate::systems::death::detach;
use crate::systems::director::update_director;
use crate::systems::economy::{Commission, affordable, update_economy};
use crate::systems::fire::{fighting_fire, update_fire};
use crate::systems::founding::{fill_village, first_name_taken, settle_villager};
use crate::systems::ghosts::update_ghosts;
use crate::systems::herald::{self, update_herald};
use crate::systems::intents::{fresh_act, leave_building, update_intents};
use crate::systems::monsters::update_monsters;
use crate::systems::movement::separate;
use crate::systems::music::update_music;
use crate::systems::needs::update_needs;
use crate::systems::perception::update_perception;
use crate::systems::quirks::update_quirks;
use crate::systems::relationships::{brawl_unlocked, daily_relationships, lethal_unlocked};
use crate::systems::routine::{block_at, update_routine};
use crate::systems::sky::{self, update_sky};
use crate::systems::stalls::update_stalls;
use crate::systems::weather::{sheltering_from_hail, update_weather};
use crate::systems::world::{MAX_PROJECTS, job_accepts, update_world};
use crate::util::{Ctx, get_monster, rel, set_anim};

const MAX_QUEUE: usize = 3;

/// Whether an intent still makes sense for this character right now.
fn intent_allowed(ctx: &mut Ctx, id: &str, intent: &Intent) -> bool {
    match &intent.action {
        IntentAction::GoTo { place_id } => find_place(place_id).is_some(),
        // A child has no trade: nothing outside a child's activities, whoever asks.
        IntentAction::Activity { activity, place_id, .. } => {
            (!ctx.cr(id).is_child() || ActivityId::CHILD_OFFERED.contains(activity))
                && place_id.as_deref().is_none_or(|p| find_place(p).is_some())
        }
        IntentAction::Talk { target_id, .. } | IntentAction::Give { target_id, .. } => {
            target_id != id && ctx.alive(target_id)
        }
        IntentAction::Fight { target_id, mode } => {
            if *mode == FightMode::Monster {
                return get_monster(ctx.state, Some(target_id)).is_some();
            }
            if target_id == id || !ctx.alive(target_id) {
                return false;
            }
            // Nobody raises a hand to a child, and no child to anyone.
            if ctx.cr(id).is_child() || ctx.cr(target_id).is_child() {
                return false;
            }
            // Being attacked always allows fighting back; starting it needs the ladder.
            if ctx.cr(target_id).intent.as_ref().is_some_and(|i| i.fights(id)) {
                return true;
            }
            if *mode == FightMode::Brawl {
                brawl_unlocked(ctx.cr(id), target_id)
            } else {
                lethal_unlocked(ctx, id, target_id)
            }
        }
        IntentAction::Flee | IntentAction::Wait { .. } | IntentAction::Nap { .. } => true,
        // Nor going in out of the weather: the weather itself sends people in.
        IntentAction::Shelter => false,
        // Murder is the sim's own doing, never a decision handed in from outside.
        IntentAction::Stalk { .. } => false,
        // Nor is the bucket chain: the fire itself calls people to it.
        IntentAction::FightFire { .. } => false,
        // Nor is fetching a child home: whoever notices goes.
        IntentAction::Fetch { .. } => false,
        // Nor a wedding or a funeral: the village is called to them.
        IntentAction::Attend { .. } => false,
        // Nor an audience: the music itself draws people over.
        IntentAction::Listen { .. } => false,
        // Carrying materials follows from gathering them; the rest needs a target that takes the job.
        // A child does no work on the world, whoever asks.
        IntentAction::WorldWork { job, target, .. } => {
            !ctx.cr(id).is_child() && *job != WorldJob::Deliver && job_accepts(ctx.state, *job, target)
        }
        // Anyone may stoop for what the dead left, while it is still there.
        IntentAction::PickUp { loot_id } => ctx.state.loot.contains_key(loot_id),
        // Anyone may make a fuss of an animal nobody is riding.
        IntentAction::Pet { animal_id } => ctx.state.animals.get(animal_id).is_some_and(|a| a.rider.is_none()),
        // A deal needs two grown villagers; whether it goes through is settled face to face.
        IntentAction::Trade { target_id, .. } => {
            target_id != id && ctx.alive(target_id) && !ctx.cr(id).is_child() && !ctx.cr(target_id).is_child()
        }
        // Paying for building needs the coin and a free pair of hands in the village.
        IntentAction::Commission { what } => {
            let c = ctx.cr(id);
            ctx.state.projects.len() < MAX_PROJECTS
                && Commission::parse(what, &c.home_id).is_some_and(|choice| affordable(ctx.state, c).contains(&choice))
        }
    }
}

fn apply_intents(ctx: &mut Ctx, id: &str, intents: Vec<Intent>) {
    let mut valid = Vec::new();
    for intent in intents {
        if intent_allowed(ctx, id, &intent) {
            valid.push(intent);
        }
    }
    valid.truncate(MAX_QUEUE);
    if valid.is_empty() {
        return;
    }
    let first = valid.remove(0);
    // Nobody talks their way out of the bucket chain, out of fetching a child home, out of shelter from the
    // hail, or leaves a wedding or a funeral; only a fight or flight comes before those.
    let c = ctx.cr(id);
    let bound = fighting_fire(c) || fetching(c) || attending(c) || sheltering_from_hail(ctx.state, c);
    if bound && !matches!(first.action, IntentAction::Fight { .. } | IntentAction::Flee) {
        return;
    }
    let thought = first.thought.clone().filter(|t| !t.is_empty());
    match &first.action {
        IntentAction::Fight { target_id, mode } => {
            start_fight(ctx, id, target_id, *mode, first.source, first.say.clone());
            if let Some(thought) = thought {
                ctx.c(id).thought = thought;
            }
            return;
        }
        IntentAction::Flee => {
            start_flee(ctx, id, first.source);
            if let Some(thought) = thought {
                ctx.c(id).thought = thought;
            }
            return;
        }
        _ => {}
    }
    detach(ctx, id);
    let tick = ctx.tick();
    let nav = ctx.state.nav.clone();
    let c = ctx.c(id);
    if c.asleep {
        leave_building(&nav, c);
    }
    c.tool = None;
    c.combat.target_id = None;
    c.combat.hit_tick = 0;
    c.intent = Some(first);
    c.queue = valid;
    c.act = fresh_act(tick);
    if c.anim.is_seated() {
        set_anim(c, AnimId::Idle, false);
    }
}

fn apply_plan(ctx: &mut Ctx, id: &str, blocks: Vec<PlanBlock>, thought: Option<String>) {
    let mut valid: Vec<PlanBlock> = blocks
        .into_iter()
        .filter(|block| {
            block.hour.is_finite()
                && block.hour >= 0.0
                && block.hour < 24.0
                && block.place_id.as_deref().is_none_or(|p| find_place(p).is_some())
        })
        .map(|block| PlanBlock { note: block.note.map(|note| utf16_truncate(&note, 120).to_owned()), ..block })
        .collect();
    valid.sort_by(|a, b| a.hour.partial_cmp(&b.hour).unwrap_or(std::cmp::Ordering::Equal));
    if valid.len() < 2 {
        return;
    }
    // A child has no trade: a block that names one becomes play.
    if ctx.cr(id).is_child() {
        for block in &mut valid {
            if !ActivityId::CHILD_OFFERED.contains(&block.activity) {
                block.activity = ActivityId::Play;
                block.place_id = None;
            }
        }
    }
    // Everyone sleeps: a plan that forgets to is given a bedtime.
    if !valid.iter().any(|block| block.activity == ActivityId::Sleep) {
        valid.push(PlanBlock { hour: 22.0, activity: ActivityId::Sleep, place_id: None, note: None });
    }
    let (day, hour) = (ctx.time.day, ctx.time.hour);
    let c = ctx.c(id);
    c.plan_block = block_at(&valid, hour);
    c.plan = valid;
    c.plan_day = day;
    c.plan_source = PlanSource::Llm;
    if let Some(thought) = thought.filter(|t| !t.is_empty()) {
        c.thought = thought.clone();
        ctx.remember(id, format!("My plan for today: {thought}"), 3.0, MemoryKind::Thought, Vec::new());
    }
}

fn apply_command(ctx: &mut Ctx, command: Command) {
    match command {
        Command::SetPlan { id, blocks, thought } => {
            if ctx.alive(&id) {
                apply_plan(ctx, &id, blocks, thought);
            }
        }
        Command::SetIntents { id, epoch, intents } => {
            // A decision made for a situation that has since changed is dropped.
            let Some(c) = ctx.state.characters.get(&id) else { return };
            if c.epoch != epoch || c.convo_id.is_some() || c.combat.yielded || c.combat.mercy_until_tick > 0 {
                return;
            }
            apply_intents(ctx, &id, intents);
        }
        Command::ConversationLines { convo_id, lines, topic, feelings, closed } => {
            add_lines(ctx, &convo_id, &lines, closed, topic.as_deref(), feelings.as_ref());
        }
        Command::ConversationLocal { convo_id } => force_local(ctx, &convo_id),
        Command::Mercy { attacker_id, spare, say } => {
            if ctx.state.characters.get(&attacker_id).is_some_and(|a| a.combat.mercy_until_tick > 0) {
                resolve_mercy(ctx, &attacker_id, spare, say);
            }
        }
        Command::Reflection { id, summary, insights, relationships } => {
            if !ctx.alive(&id) {
                return;
            }
            let summary = summary.trim();
            if !summary.is_empty() {
                ctx.c(&id).summary = utf16_truncate(summary, 600).to_owned();
            }
            for insight in insights.iter().take(4) {
                let insight = insight.trim();
                if !insight.is_empty() {
                    ctx.remember(&id, utf16_truncate(insight, 240), 7.0, MemoryKind::Reflection, Vec::new());
                }
            }
            for entry in relationships {
                let note = entry.summary.trim();
                if ctx.alive(&entry.id) && entry.id != id && !note.is_empty() {
                    rel(ctx.c(&id), &entry.id).summary = utf16_truncate(note, 200).to_owned();
                }
            }
            let day = ctx.time.day;
            ctx.c(&id).last_reflection_day = day;
        }
        Command::Thought { id, text } => {
            if let Some(c) = ctx.state.characters.get_mut(&id) {
                c.thought = utf16_truncate(&text, 240).to_owned();
            }
        }
        Command::ClearStimuli { id, through_tick } => {
            if let Some(c) = ctx.state.characters.get_mut(&id) {
                c.stimuli.retain(|stimulus| stimulus.tick > through_tick);
            }
        }
        Command::Tuning { tuning } => {
            let t = &mut ctx.state.tuning;
            if let Some(chatter) = tuning.chatter {
                t.chatter = chatter;
            }
            if let Some(ticks) = tuning.script_wait_ticks {
                t.script_wait_ticks = ticks;
            }
            if let Some(ticks) = tuning.mercy_wait_ticks {
                t.mercy_wait_ticks = ticks;
            }
        }
        Command::SetBillboard { board_id, notice } => {
            if find_billboard(&board_id).is_none() {
                return;
            }
            let notice = notice.and_then(|notice| {
                let text = clean_text(&notice.text)?;
                Some(BoardNotice { text: utf16_truncate(&text, TEXT_CAP).to_owned(), tone: notice.tone, day: None })
            });
            match notice {
                Some(notice) => {
                    ctx.state.billboards.insert(board_id, notice);
                }
                None => {
                    ctx.state.billboards.shift_remove(&board_id);
                }
            }
        }
        Command::WriteBillboard { board_id, day, text } => {
            let Some(index) = board_index(&board_id) else { return };
            if wants_words(ctx.state, index) != Some(day) {
                return;
            }
            let Some(text) = clean_text(&text) else { return };
            let notice = BoardNotice {
                text: utf16_truncate(&text, TEXT_CAP).to_owned(),
                tone: day_tone(ctx.state, index),
                day: Some(day),
            };
            ctx.state.billboards.insert(board_id, notice);
        }
        Command::Audience { crowd } => sky::set_audience(ctx, crowd),
        Command::WriteHerald { day, headline, stories, gossip } => {
            herald::publish(ctx, HeraldIssue { day, headline, stories, gossip, by_model: true });
        }
        Command::BillboardReaction { id, board_id, reaction, say, thought, follow } => {
            let Some(c) = ctx.state.characters.get(&id) else { return };
            let Some(index) = reading(c) else { return };
            if board_index(&board_id) != Some(index) || has_read(ctx.state, c, index) {
                return;
            }
            // Only a board that hints at the watchers beyond the world can be taken as one.
            let reaction = match reaction {
                _ if shown(ctx.state, index).watchers => reaction,
                BoardReaction::Shocked => BoardReaction::Uneasy,
                BoardReaction::Intrigued => BoardReaction::Scoff,
                BoardReaction::Angry => BoardReaction::Annoyed,
                reaction => reaction,
            };
            let say = say.and_then(|s| clean_text(&s)).map(|s| utf16_truncate(&s, 160).to_owned());
            let thought = thought.and_then(|s| clean_text(&s)).map(|s| utf16_truncate(&s, 240).to_owned());
            let ticks = react(ctx, &id, index, reaction, say, thought, follow);
            let tick = ctx.tick();
            let c = ctx.c(&id);
            c.act.until_tick = c.act.until_tick.min(tick + ticks.min(160));
        }
        Command::Settle(villager) => settle_villager(ctx, &villager),
        Command::FoundVillage => fill_village(ctx),
        Command::CallRaid => {
            crate::systems::raids::call_raid(ctx);
        }
        Command::CallDragon => {
            crate::systems::dragon::call_dragon(ctx);
        }
        Command::NewcomerDraft(villager) => {
            if !first_name_taken(ctx.state, &villager.name) {
                ctx.state.director.newcomer_draft = Some(villager);
            }
        }
        Command::NewcomerLocal => {
            // Only once asked: an unasked-for refusal must not shorten a future wait.
            if ctx.state.director.newcomer_deadline_tick != 0 {
                ctx.state.director.newcomer_deadline_tick = ctx.tick();
            }
        }
        Command::CeremonySpeech { ceremony_id, speaker_id, text } => {
            ceremonies::hold_speech(ctx, &ceremony_id, &speaker_id, text);
        }
        Command::NameAnimal { animal_id, name } => animals::name_pet(ctx, &animal_id, &name),
    }
}

/// Steps a state forward; holds the commands for the next tick.
pub struct Sim {
    pub state: SimState,
    pending: Vec<Command>,
}

impl Sim {
    /// A running sim from a state, new or saved. The walkability overlay is derived from the state,
    /// so it is built here rather than saved.
    pub fn new(mut state: SimState) -> Self {
        crate::nav::refresh(&mut state);
        Self { state, pending: Vec::new() }
    }

    pub fn enqueue(&mut self, command: Command) {
        self.pending.push(command);
    }

    /// Commands waiting for the next tick.
    pub fn pending(&self) -> &[Command] {
        &self.pending
    }

    pub fn step(&mut self) -> Vec<SimEvent> {
        let previous_day = game_time(self.state.tick).day;
        self.state.tick += 1;
        let mut events = Vec::new();
        let commands = std::mem::take(&mut self.pending);
        let mut ctx = Ctx::new(&mut self.state, &mut events);

        for command in commands {
            apply_command(&mut ctx, command);
        }

        if ctx.time.day != previous_day {
            daily_relationships(&mut ctx);
            ceremonies::daily(&mut ctx);
            daily_children(&mut ctx);
            let day = ctx.time.day;
            ctx.emit(SimEvent::NewDay { day });
            ctx.emit(SimEvent::SavePoint { reason: "new day".into() });
        }
        if (ctx.time.minute_of_day - 360.0).abs() < 0.026 && ctx.tick() > 1 {
            let day = ctx.time.day;
            ctx.chronicle(ChronicleKind::World, format!("Day {day} dawns over the valley."), Vec::new());
        }

        update_weather(&mut ctx);
        update_director(&mut ctx);
        update_routine(&mut ctx);
        update_quirks(&mut ctx);
        update_children(&mut ctx);
        update_intents(&mut ctx);
        update_music(&mut ctx);
        update_conversations(&mut ctx);
        update_monsters(&mut ctx);
        update_ghosts(&mut ctx);
        update_animals(&mut ctx);
        update_fire(&mut ctx);
        update_ceremonies(&mut ctx);
        update_cases(&mut ctx);
        update_herald(&mut ctx);
        update_sky(&mut ctx);
        update_ambitions(&mut ctx);
        update_world(&mut ctx);
        update_economy(&mut ctx);
        update_perception(&mut ctx);
        update_blocking(&mut ctx);
        separate(ctx.state);
        update_stalls(&mut ctx);
        update_needs(&mut ctx);
        events
    }
}
