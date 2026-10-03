//! The tools a character's mind can call (port of `src/brain/tools/definitions.ts`).
//! Schemas are static (valid people, places and activities are listed in the
//! prompt and checked afterwards), so servers can cache the tool block.

use std::sync::LazyLock;

use fsw_world::places::ActivityId;
use serde_json::{Value, json};

use crate::llm::types::ToolDef;

fn say() -> Value {
    json!({ "type": "string", "description": "Optional. What you say aloud as you begin: one short sentence in your own voice, no quotation marks." })
}

fn thought() -> Value {
    json!({ "type": "string", "description": "Optional. One private sentence, in the first person, on why." })
}

fn person() -> Value {
    json!({ "type": "string", "description": "First name of a living person named in the message." })
}

fn feeling(description: &str) -> Value {
    json!({ "type": "string", "enum": ["much_worse", "worse", "same", "better", "much_better"], "description": description })
}

fn activity_ids() -> Vec<&'static str> {
    ActivityId::OFFERED.iter().map(|id| id.as_str()).collect()
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> ToolDef {
    ToolDef::new(
        name,
        description,
        json!({ "type": "object", "properties": properties, "required": required, "additionalProperties": false }),
    )
}

pub struct Tools {
    pub go_to: ToolDef,
    pub do_activity: ToolDef,
    pub talk_to: ToolDef,
    pub give: ToolDef,
    pub continue_: ToolDef,
    pub brawl: ToolDef,
    pub attack: ToolDef,
    pub fight: ToolDef,
    pub flee: ToolDef,
    pub set_plan: ToolDef,
    pub conversation: ToolDef,
    pub reply: ToolDef,
    pub spare: ToolDef,
    pub finish: ToolDef,
    pub reflection: ToolDef,
    pub billboard: ToolDef,
}

pub static TOOLS: LazyLock<Tools> = LazyLock::new(|| Tools {
    go_to: tool(
        "go_to",
        "Walk to a place.",
        json!({ "place": { "type": "string", "description": "A place named in the message." }, "say": say(), "thought": thought() }),
        &["place"],
    ),
    do_activity: tool(
        "do_activity",
        "Go and do one of the listed activities for a while.",
        json!({
            "activity": { "type": "string", "enum": activity_ids(), "description": "The activity id." },
            "say": say(),
            "thought": thought(),
        }),
        &["activity"],
    ),
    talk_to: tool(
        "talk_to",
        "Walk up to someone and start a conversation.",
        json!({
            "person": person(),
            "message": { "type": "string", "description": "Your opening line: one or two short sentences in your own voice." },
            "tone": {
                "type": "string",
                "enum": ["friendly", "romantic", "hostile", "apology"],
                "description": "friendly: ordinary talk. romantic: flirting or confessing feelings. hostile: confronting them over a grievance. apology: trying to make amends.",
            },
            "thought": thought(),
        }),
        &["person", "message", "tone"],
    ),
    give: tool(
        "give",
        "Walk up to someone and give them something you are carrying.",
        json!({ "person": person(), "item": { "type": "string", "enum": ["food", "potion", "gift"] }, "say": say(), "thought": thought() }),
        &["person", "item"],
    ),
    continue_: tool("continue", "Carry on with what you are already doing.", json!({ "thought": thought() }), &[]),
    brawl: tool(
        "brawl",
        "Start a fist fight with someone you have a feud with. Nobody dies, but it will be remembered.",
        json!({ "person": person(), "say": say(), "thought": thought() }),
        &["person"],
    ),
    attack: tool(
        "attack",
        "Draw your weapon on another villager, meaning to kill. Only when hatred has gone as far as it can go; the whole village will know.",
        json!({ "person": person(), "say": say(), "thought": thought() }),
        &["person"],
    ),
    fight: tool(
        "fight",
        "Stand and fight the danger in front of you.",
        json!({ "say": say(), "thought": thought() }),
        &[],
    ),
    flee: tool("flee", "Run for home and hide until it is safe.", json!({ "say": say(), "thought": thought() }), &[]),
    set_plan: tool(
        "set_plan",
        "Set your plan for today.",
        json!({
            "blocks": {
                "type": "array",
                "minItems": 3,
                "maxItems": 8,
                "description": "The day in order, from waking to bedtime. Include meals and end with sleep.",
                "items": {
                    "type": "object",
                    "properties": {
                        "hour": { "type": "number", "description": "Hour of day the block starts, 0-24 (for example 13.5 for half past one)." },
                        "activity": { "type": "string", "enum": activity_ids() },
                        "note": { "type": "string", "description": "Optional. A few words on why or with whom." },
                    },
                    "required": ["hour", "activity"],
                    "additionalProperties": false,
                },
            },
            "thought": { "type": "string", "description": "One sentence, in the first person, on what you want from today." },
        }),
        &["blocks"],
    ),
    conversation: tool(
        "conversation",
        "Write the rest of the conversation.",
        json!({
            "lines": {
                "type": "array",
                "minItems": 2,
                "maxItems": 6,
                "description": "The lines that follow the opening line, alternating speakers, starting with the person who was spoken to.",
                "items": {
                    "type": "object",
                    "properties": {
                        "speaker": { "type": "string", "description": "First name of the speaker." },
                        "text": { "type": "string", "description": "What they say: one or two short sentences, no stage directions." },
                    },
                    "required": ["speaker", "text"],
                    "additionalProperties": false,
                },
            },
            "topic": { "type": "string", "description": "What the conversation was about, in a few words." },
            "initiator_feels": feeling("How the one who started the conversation now feels about the other, compared with before."),
            "other_feels": feeling("How the other now feels about the one who started it, compared with before."),
        }),
        &["lines", "topic", "initiator_feels", "other_feels"],
    ),
    reply: tool(
        "reply",
        "Say your next line in the conversation.",
        json!({
            "text": { "type": "string", "description": "What you say: one or two short sentences in your own voice, no stage directions." },
            "feeling": feeling("How you now feel about the person you are talking to, compared with before this conversation."),
            "end": { "type": "boolean", "description": "True if this line ends the conversation." },
        }),
        &["text", "feeling", "end"],
    ),
    spare: tool("spare", "Let your beaten opponent live.", json!({ "say": say(), "thought": thought() }), &[]),
    finish: tool(
        "finish",
        "Kill your beaten opponent. This cannot be undone.",
        json!({ "say": say(), "thought": thought() }),
        &[],
    ),
    reflection: tool(
        "reflection",
        "Record what you take from the day.",
        json!({
            "summary": { "type": "string", "description": "Two or three sentences, first person: who you are and where things stand for you now." },
            "insights": {
                "type": "array",
                "minItems": 1,
                "maxItems": 3,
                "items": { "type": "string" },
                "description": "One to three conclusions you have drawn (about yourself, others, or what to do), each one sentence in the first person.",
            },
            "relationships": {
                "type": "array",
                "maxItems": 4,
                "description": "For up to four people who mattered today: how you now see them.",
                "items": {
                    "type": "object",
                    "properties": { "person": person(), "opinion": { "type": "string", "description": "One sentence, first person." } },
                    "required": ["person", "opinion"],
                    "additionalProperties": false,
                },
            },
        }),
        &["summary", "insights"],
    ),
    billboard: tool(
        "react_to_billboard",
        "Say how you take what the billboard says.",
        json!({
            "reaction": {
                "type": "string",
                "enum": ["laugh", "love", "scoff", "annoyed", "uneasy"],
                "description": "laugh: it amuses you. love: it moves or cheers you. scoff: you think little of it. annoyed: it irritates you. uneasy: it unsettles you.",
            },
            "say": { "type": "string", "description": "Optional. What you mutter or say aloud about it: one short sentence in your own voice, no quotation marks." },
            "thought": thought(),
        }),
        &["reaction"],
    ),
});
