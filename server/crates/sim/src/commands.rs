//! Everything the outside world (the LLM brain, the admin) may ask of the sim.
//! Commands are queued and applied at the start of the next tick, so the sim
//! stays deterministic for a given command stream.

use serde::{Deserialize, Serialize};

use crate::state::{BoardNotice, BoardReaction, ConversationLine, Feeling, Id, Intent, Map, NewVillager, PlanBlock};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TuningPatch {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chatter: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script_wait_ticks: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mercy_wait_ticks: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RelationshipNote {
    pub id: Id,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum Command {
    SetPlan {
        id: Id,
        blocks: Vec<PlanBlock>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought: Option<String>,
    },
    /// Replaces what the character is doing; ignored if their situation changed since `epoch`.
    SetIntents {
        id: Id,
        epoch: i64,
        intents: Vec<Intent>,
    },
    ConversationLines {
        convo_id: Id,
        lines: Vec<ConversationLine>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        topic: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feelings: Option<Map<Feeling>>,
        closed: bool,
    },
    /// Stop waiting for LLM lines and finish the conversation with local ones.
    ConversationLocal {
        convo_id: Id,
    },
    Mercy {
        attacker_id: Id,
        spare: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        say: Option<String>,
    },
    Reflection {
        id: Id,
        summary: String,
        insights: Vec<String>,
        relationships: Vec<RelationshipNote>,
    },
    Thought {
        id: Id,
        text: String,
    },
    /// The brain has taken these stimuli into a decision; drop everything up to and including `through_tick`.
    ClearStimuli {
        id: Id,
        through_tick: i64,
    },
    Tuning {
        tuning: TuningPatch,
    },
    /// Puts text up on a billboard; `None` returns it to the daily slogans.
    SetBillboard {
        board_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        notice: Option<BoardNotice>,
    },
    /// How someone reading a billboard takes it; ignored unless they are still reading it and have not reacted.
    BillboardReaction {
        id: Id,
        board_id: String,
        reaction: BoardReaction,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        say: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        thought: Option<String>,
        /// whether they go and do what the board urges; `None` leaves it to chance, as without a model
        #[serde(default, skip_serializing_if = "Option::is_none")]
        follow: Option<bool>,
    },
    /// One more founder for a village still being settled; ignored once it is founded or when the
    /// first name is taken.
    Settle(Box<NewVillager>),
    /// Fills a village still being settled with villagers the sim invents (no model, or none that answers).
    FoundVillage,
    /// The server's description of the next newcomer, held until an arrival is due; dropped when the
    /// first name is taken.
    NewcomerDraft(Box<NewVillager>),
    /// No description is coming for the newcomer that was asked for: the sim invents one at the next chance.
    NewcomerLocal,
    /// An admin calls an orc raid: it comes as soon as it may (by day, with no house burning and room on the
    /// road). Ignored while the village is being founded, while a band is in the valley or one is already called.
    CallRaid,
    /// Words the model wrote for someone to say at a ceremony; ignored unless the ceremony still stands and
    /// the speaker still speaks at it.
    CeremonySpeech {
        ceremony_id: Id,
        speaker_id: Id,
        text: String,
    },
}
