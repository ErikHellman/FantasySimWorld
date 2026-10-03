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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Projectile {
    Arrow,
    Bolt,
    Magic,
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
    /// Someone has stepped up to a billboard and started reading it.
    BillboardRead {
        id: Id,
        board_id: String,
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
            Self::NewDay { .. } => "new_day",
            Self::ConversationStarted { .. } => "conversation_started",
            Self::ConversationWaiting { .. } => "conversation_waiting",
            Self::ConversationEnded { .. } => "conversation_ended",
            Self::MercyRequest { .. } => "mercy_request",
            Self::Woke { .. } => "woke",
            Self::SavePoint { .. } => "save_point",
            Self::BillboardRead { .. } => "billboard_read",
        }
    }
}
