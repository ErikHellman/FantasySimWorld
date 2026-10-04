//! Everything the outside world (the LLM brain, the admin) may ask of the sim.
//! Commands are queued and applied at the start of the next tick, so the sim
//! stays deterministic for a given command stream.

use serde::{Deserialize, Serialize};

use fsw_core::Vec2;

use crate::state::{
    BoardNotice, BoardReaction, ConversationLine, Feeling, Id, Intent, ItemId, Map, MonsterKind, PlanBlock,
};

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

/// Something a viewer has paid for; see `systems::viewer_events`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", rename_all_fields = "camelCase")]
pub enum ViewerEvent {
    /// Puts one item in a villager's pack.
    Gift { character_id: Id, item: ItemId },
    /// Raises one of the dead (never a necromancer) at the walkable spot nearest `pos`.
    Monster { monster: MonsterKind, pos: Vec2 },
    /// A wave of the dead from the graveyard.
    Raid,
    /// A necromancer in the graveyard.
    Necromancer,
    /// Closes every villager's wounds.
    Heal,
    /// Fills every villager's belly.
    Feast,
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
    },
    /// A viewer's doing, by the name the villagers will know them by; ignored if it can no longer happen.
    ViewerEvent {
        by: String,
        event: ViewerEvent,
    },
}
