//! One-shot happenings of a tick, for the server's stream, the brain and the
//! chronicle. Continuous things (who is where, doing what) are read from the state.

use fsw_core::Vec2;
use serde::{Deserialize, Serialize};

use crate::state::{ChronicleEntry, Feeling, Id, Map, TalkTone};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Emote {
    Heart,
    Anger,
    Alert,
    Thinking,
    Sleep,
    Grief,
    Laugh,
    BrokenHeart,
    Gift,
    /// a note over someone singing or playing
    Music,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Projectile {
    Arrow,
    Bolt,
    Magic,
    /// an apple thrown by a child; it startles, and does no harm
    Food,
    /// the dragon's breath, poured onto the ground below it
    Fire,
}

/// Where an orc raid has got to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RaidPhase {
    /// the band has appeared on the west road
    Coming,
    /// it has had enough and makes for the road
    Leaving,
    /// the last of it is gone
    Gone,
}

/// How a blow ended for the one it was aimed at.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HitOutcome {
    #[default]
    Hit,
    /// turned aside: a quarter of the damage
    Blocked,
    /// stepped out of: no damage at all
    Dodged,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum SimEvent {
    Speech {
        speaker_id: Id,
        to_id: Option<Id>,
        text: String,
        duration_ticks: i64,
        loud: bool,
    },
    Emote {
        id: Id,
        emote: Emote,
    },
    Attack {
        attacker_id: Id,
        target_id: Id,
        ranged: Option<Projectile>,
        from: Vec2,
        to: Vec2,
    },
    Hit {
        attacker_id: Id,
        target_id: Id,
        damage: f64,
        pos: Vec2,
        fatal: bool,
        #[serde(default)]
        outcome: HitOutcome,
    },
    Death {
        id: Id,
        name: String,
        pos: Vec2,
        monster: bool,
    },
    MonsterSpawn {
        id: Id,
        pos: Vec2,
    },
    Chronicle {
        entry: ChronicleEntry,
    },
    Arrival {
        id: Id,
    },
    /// A wedding or a funeral has been arranged; the brain may ask for its speeches.
    CeremonyScheduled {
        ceremony_id: Id,
    },
    NewDay {
        day: i64,
    },
    ConversationStarted {
        convo_id: Id,
        a: Id,
        b: Id,
        tone: TalkTone,
    },
    ConversationWaiting {
        convo_id: Id,
    },
    ConversationEnded {
        convo_id: Id,
        a: Id,
        b: Id,
        feelings: Map<Feeling>,
    },
    MercyRequest {
        attacker_id: Id,
        victim_id: Id,
    },
    Woke {
        id: Id,
    },
    SavePoint {
        reason: String,
    },
    /// An orc war band has come up the road, turned for home, or gone.
    Raid {
        phase: RaidPhase,
    },
    /// Someone has stepped up to a billboard and started reading it.
    BillboardRead {
        id: Id,
        board_id: String,
    },
    /// An arrival is due and the sim would like the server to describe the newcomer; past
    /// `deadline_tick` it invents one itself.
    NewcomerWanted {
        deadline_tick: i64,
    },
}

impl SimEvent {
    /// The `type` tag, as the TypeScript sim names it.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Speech { .. } => "speech",
            Self::Emote { .. } => "emote",
            Self::Attack { .. } => "attack",
            Self::Hit { .. } => "hit",
            Self::Death { .. } => "death",
            Self::MonsterSpawn { .. } => "monster_spawn",
            Self::Chronicle { .. } => "chronicle",
            Self::Arrival { .. } => "arrival",
            Self::CeremonyScheduled { .. } => "ceremony_scheduled",
            Self::NewDay { .. } => "new_day",
            Self::ConversationStarted { .. } => "conversation_started",
            Self::ConversationWaiting { .. } => "conversation_waiting",
            Self::ConversationEnded { .. } => "conversation_ended",
            Self::MercyRequest { .. } => "mercy_request",
            Self::Woke { .. } => "woke",
            Self::SavePoint { .. } => "save_point",
            Self::Raid { .. } => "raid",
            Self::BillboardRead { .. } => "billboard_read",
            Self::NewcomerWanted { .. } => "newcomer_wanted",
        }
    }
}
