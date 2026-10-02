//! Tolerant reading of model replies (port of `src/brain/llm/parse.ts`).
//!
//! Behaviour follows the TypeScript exactly, including JavaScript's notions of
//! whitespace, `\w` (ASCII only), `.` (no line terminators) and string length
//! (UTF-16 units). Two differences: object keys come back sorted (serde_json
//! maps are ordered by key, JavaScript keeps insertion order), and a "key:
//! value" line after a bare tool name only fills keys the schema itself lists
//! (the TypeScript `in` check also accepted inherited names like `toString`).

use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value};

use super::types::{ToolCall, ToolDef};
use crate::settings::js_trim;

/// JavaScript's `\s`.
const WS: &str = r"[\t\n\x0B\x0C\r \xA0\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";
/// JavaScript's `.` without the `s` flag.
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
/// `think` or `thinking`, ASCII case-insensitive like a JavaScript `/i` regex.
const THINK: &str = "[tT][hH][iI][nN][kK](?:[iI][nN][gG])?";

fn regex(pattern: &str) -> Regex {
    Regex::new(pattern).expect("static regex")
}

static THINK_BLOCK: LazyLock<Regex> = LazyLock::new(|| regex(&format!("(?s)<{THINK}>.*?</{THINK}>")));
static THINK_UNOPENED: LazyLock<Regex> = LazyLock::new(|| regex(&format!("(?s)^.*?</{THINK}>")));
static TRAILING_COMMA: LazyLock<Regex> = LazyLock::new(|| regex(&format!(r",{WS}*([}}\]])")));
static SINGLE_QUOTED_KEY: LazyLock<Regex> = LazyLock::new(|| regex(&format!(r"([{{,]{WS}*)'([^']+)'{WS}*:")));
static SINGLE_QUOTED_VALUE: LazyLock<Regex> = LazyLock::new(|| regex(&format!(r":{WS}*'([^']*)'")));
static FUNCTION_PREFIX: LazyLock<Regex> = LazyLock::new(|| regex("^functions?[.:]"));
static SCRIPT_LINE: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"^\**([A-Z][A-Za-z0-9_'-]{{1,20}})\**{WS}*:{WS}*({DOT}+)$")));
static SPEECH_QUOTES: LazyLock<Regex> = LazyLock::new(|| regex(r#"^["“]|["”]$"#));
static NOT_NAME: LazyLock<Regex> = LazyLock::new(|| regex("[^a-z_]"));
static ARG_LINE: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r#"^["']?([A-Za-z0-9_]+)["']?{WS}*[:=]{WS}*({DOT}+)$"#)));
static ARG_QUOTES: LazyLock<Regex> = LazyLock::new(|| regex(r#"^["']|["'],?$"#));
static SPEAKER_PREFIX: LazyLock<Regex> = LazyLock::new(|| regex(&format!(r"^(You|[A-Za-z0-9_]+):{WS}*")));

/// Removes reasoning blocks some models put in front of their answer.
pub fn strip_thinking(text: &str) -> String {
    let closed = THINK_BLOCK.replace_all(text, "");
    // an unopened block (the opening tag was cut off)
    let unopened = THINK_UNOPENED.replacen(&closed, 1, "");
    js_trim(&unopened).to_string()
}

/// Parses JSON that may be slightly malformed. Blank text is `None`.
pub fn loose_json_str(text: &str) -> Option<Value> {
    let text = js_trim(text);
    if text.is_empty() {
        return None;
    }
    if let Ok(value) = serde_json::from_str(text) {
        return Some(value);
    }
    // Trailing commas and single quotes are the usual offenders.
    let repaired = TRAILING_COMMA.replace_all(text, "${1}");
    let repaired = SINGLE_QUOTED_KEY.replace_all(&repaired, "${1}\"${2}\":");
    let repaired = SINGLE_QUOTED_VALUE.replace_all(&repaired, ": \"${1}\"");
    serde_json::from_str(&repaired).ok()
}

/// Parses JSON that may be a string, already a value, or slightly malformed. `null` is `None`.
pub fn loose_json(value: Option<&Value>) -> Option<Value> {
    match value? {
        Value::Null => None,
        Value::String(text) => loose_json_str(text),
        other => Some(other.clone()),
    }
}

/// Every balanced top-level {...} or [...] block in a text, in order.
pub fn json_blocks(text: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut depth = 0usize;
    let mut start: Option<usize> = None;
    let mut in_string = false;
    let mut escaped = false;
    // Every character that matters is ASCII, so byte offsets are character boundaries.
    for (i, &ch) in text.as_bytes().iter().enumerate() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == b'\\' {
                escaped = true;
            } else if ch == b'"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            b'"' => in_string = true,
            b'{' | b'[' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            b'}' | b']' if depth > 0 => {
                depth -= 1;
                if depth == 0
                    && let Some(from) = start.take()
                {
                    blocks.push(&text[from..=i]);
                }
            }
            _ => {}
        }
    }
    blocks
}

fn as_object(value: Option<&Value>) -> Option<&Map<String, Value>> {
    value.and_then(Value::as_object)
}

/// The first of these keys whose value is neither missing nor null (JavaScript's `a ?? b ?? c`).
fn first_present<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| object.get(*key).filter(|value| !value.is_null()))
}

fn or_else<'a>(first: Option<&'a Value>, fallback: Option<&'a Value>) -> Option<&'a Value> {
    first.or(fallback.filter(|value| !value.is_null()))
}

fn args_from(value: Option<&Value>) -> Map<String, Value> {
    match loose_json(value) {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    }
}

fn name_match(name: Option<&Value>, tools: &[ToolDef]) -> Option<String> {
    let name = name?.as_str()?;
    let lower = js_trim(name).to_lowercase();
    let wanted = FUNCTION_PREFIX.replace(&lower, "");
    tools.iter().find(|tool| tool.name.to_lowercase() == wanted).map(|tool| tool.name.clone())
}

const CALL_KEYS: [&str; 11] =
    ["name", "tool", "tool_name", "action", "function", "arguments", "args", "parameters", "input", "type", "id"];

/// Recognises the shapes models use for a tool call when they write it as plain JSON.
fn call_from(object: &Map<String, Value>, tools: &[ToolDef]) -> Option<ToolCall> {
    let function = as_object(object.get("function"));
    let named =
        or_else(first_present(object, &["name", "tool", "tool_name", "action"]), function.and_then(|f| f.get("name")));
    if let Some(name) = name_match(named, tools) {
        let given = or_else(
            first_present(object, &["arguments", "args", "parameters", "input"]),
            function.and_then(|f| f.get("arguments")),
        );
        let args = args_from(given);
        // {"tool": "go_to", "place": "tavern"}: arguments given inline.
        if args.is_empty() {
            let inline = object
                .iter()
                .filter(|(key, _)| !CALL_KEYS.contains(&key.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()));
            return Some(ToolCall { name, args: inline.collect() });
        }
        return Some(ToolCall { name, args });
    }
    // {"go_to": {"place": "tavern"}}
    if object.len() == 1 {
        let (key, value) = object.iter().next()?;
        let only = name_match(Some(&Value::String(key.clone())), tools)?;
        let args = value.as_object()?;
        return Some(ToolCall { name: only, args: args.clone() });
    }
    None
}

fn utf16_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// Last resort for models that answer in plain text instead of calling the tool:
/// - a play script ("Kael: ...", "Elowen: ...") when the conversation tool was offered;
/// - a bare tool name followed by "key: value" lines ("flee", "say: Run!");
/// - plain prose when the only tool offered takes a single line of speech.
fn calls_from_prose(text: &str, tools: &[ToolDef]) -> Vec<ToolCall> {
    let lines: Vec<&str> = text.split('\n').map(js_trim).filter(|line| !line.is_empty()).collect();
    let Some(first_line) = lines.first() else { return Vec::new() };

    if tools.iter().any(|tool| tool.name == "conversation") {
        let script: Vec<Value> = lines
            .iter()
            .filter_map(|line| SCRIPT_LINE.captures(line))
            .map(|caps| {
                let text = SPEECH_QUOTES.replace_all(&caps[2], "");
                serde_json::json!({ "speaker": &caps[1], "text": text })
            })
            .collect();
        if script.len() >= 2 {
            return vec![ToolCall::new("conversation", serde_json::json!({ "lines": script, "topic": "" }))];
        }
    }

    let first = NOT_NAME.replace_all(&first_line.to_lowercase(), "").into_owned();
    if let Some(named) = tools.iter().find(|tool| tool.name == first) {
        let mut args = Map::new();
        for line in &lines[1..] {
            let Some(caps) = ARG_LINE.captures(line) else { continue };
            if !named.has_property(&caps[1]) {
                continue;
            }
            let value = ARG_QUOTES.replace_all(&caps[2], "");
            let value = match value.as_ref() {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                other => Value::String(other.to_string()),
            };
            args.insert(caps[1].to_string(), value);
        }
        return vec![ToolCall { name: named.name.clone(), args }];
    }

    if let [only] = tools
        && only.name == "reply"
        && utf16_len(text) <= 400
        && !text.contains('{')
    {
        let joined = lines.join(" ");
        let spoken = SPEAKER_PREFIX.replace(&joined, "");
        return vec![ToolCall::new("reply", serde_json::json!({ "text": spoken, "feeling": "same", "end": false }))];
    }
    Vec::new()
}

fn visit(value: &Value, tools: &[ToolDef], calls: &mut Vec<ToolCall>) {
    if let Value::Array(items) = value {
        for item in items {
            visit(item, tools, calls);
        }
        return;
    }
    let Value::Object(object) = value else { return };
    if let Some(Value::Array(wrapped)) = first_present(object, &["calls", "tool_calls", "actions", "steps"]) {
        for item in wrapped {
            visit(item, tools, calls);
        }
        return;
    }
    if let Some(call) = call_from(object, tools) {
        calls.push(call);
    }
}

/// Finds tool calls written into the text of a reply.
pub fn calls_from_text(text: &str, tools: &[ToolDef]) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    let cleaned = strip_thinking(text);
    for block in json_blocks(&cleaned) {
        if let Some(value) = loose_json_str(block) {
            visit(&value, tools, &mut calls);
        }
    }
    if calls.is_empty() { calls_from_prose(&cleaned, tools) } else { calls }
}

/// Tool calls from an OpenAI-style assistant message, falling back to its text.
pub fn calls_from_message(message: &Map<String, Value>, tools: &[ToolDef]) -> Vec<ToolCall> {
    let mut calls = Vec::new();
    if let Some(Value::Array(native)) = message.get("tool_calls") {
        for entry in native {
            let function = as_object(entry.get("function"));
            let Some(name) = name_match(function.and_then(|f| f.get("name")), tools) else { continue };
            calls.push(ToolCall { name, args: args_from(function.and_then(|f| f.get("arguments"))) });
        }
    }
    if !calls.is_empty() {
        return calls;
    }
    let legacy = as_object(message.get("function_call"));
    if let Some(name) = name_match(legacy.and_then(|f| f.get("name")), tools) {
        return vec![ToolCall { name, args: args_from(legacy.and_then(|f| f.get("arguments"))) }];
    }
    match message.get("content") {
        Some(Value::String(content)) => calls_from_text(content, tools),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use serde_json::json;

    use super::*;

    fn tool(name: &str, properties: Value) -> ToolDef {
        ToolDef::new(name, "", json!({ "type": "object", "properties": properties }))
    }

    fn tools() -> Vec<ToolDef> {
        vec![tool("go_to", json!({ "place": { "type": "string" } })), tool("continue", json!({}))]
    }

    fn call(name: &str, args: Value) -> ToolCall {
        ToolCall::new(name, args)
    }

    fn message(value: Value) -> Map<String, Value> {
        value.as_object().unwrap().clone()
    }

    #[test]
    fn strips_reasoning_blocks_including_one_whose_opening_tag_was_lost() {
        assert_eq!(strip_thinking(r#"<think>hmm</think>{"a":1}"#), r#"{"a":1}"#);
        assert_eq!(strip_thinking("still thinking...</think> done"), "done");
        assert_eq!(strip_thinking("<THINKING>a\nb</Thinking> x <think>c</think>y"), "x y");
        assert_eq!(strip_thinking("\u{FEFF} keep \u{85}"), "keep \u{85}");
    }

    #[test]
    fn repairs_slightly_broken_json() {
        assert_eq!(loose_json_str(r#"{"a": 1,}"#), Some(json!({ "a": 1 })));
        assert_eq!(loose_json_str("{'place': 'tavern'}"), Some(json!({ "place": "tavern" })));
        assert_eq!(loose_json(Some(&json!({ "already": true }))), Some(json!({ "already": true })));
        assert_eq!(loose_json_str("not json"), None);
        assert_eq!(loose_json(Some(&Value::Null)), None);
        assert_eq!(loose_json(None), None);
        assert_eq!(loose_json_str("  "), None);
    }

    #[test]
    fn finds_balanced_json_blocks_in_prose() {
        assert_eq!(
            json_blocks(r#"I will go. {"name":"go_to","arguments":{"place":"a } b"}} ok [1,2]"#),
            vec![r#"{"name":"go_to","arguments":{"place":"a } b"}}"#, "[1,2]"]
        );
        assert_eq!(json_blocks(r#"} ] {"a":"\"}"} {unclosed"#), vec![r#"{"a":"\"}"}"#]);
    }

    #[test]
    fn reads_native_tool_calls_with_string_or_object_arguments() {
        let reply = message(json!({
            "content": null,
            "tool_calls": [
                { "type": "function", "function": { "name": "go_to", "arguments": "{\"place\":\"tavern\"}" } },
                { "type": "function", "function": { "name": "continue", "arguments": {} } },
                { "type": "function", "function": { "name": "unknown_tool", "arguments": "{}" } },
            ],
        }));
        assert_eq!(
            calls_from_message(&reply, &tools()),
            vec![call("go_to", json!({ "place": "tavern" })), call("continue", json!({}))]
        );
    }

    #[test]
    fn reads_a_legacy_function_call() {
        let reply =
            message(json!({ "function_call": { "name": "Functions.GO_TO", "arguments": "{\"place\":\"x\"}" } }));
        assert_eq!(calls_from_message(&reply, &tools()), vec![call("go_to", json!({ "place": "x" }))]);
    }

    #[test]
    fn falls_back_to_tool_calls_written_as_text() {
        let tools = tools();
        let fenced = message(
            json!({ "content": "```json\n{\"name\": \"go_to\", \"arguments\": {\"place\": \"market\"}}\n```" }),
        );
        assert_eq!(calls_from_message(&fenced, &tools), vec![call("go_to", json!({ "place": "market" }))]);
        assert_eq!(
            calls_from_text(r#"<tool_call>{"tool":"go_to","place":"smithy"}</tool_call>"#, &tools),
            vec![call("go_to", json!({ "place": "smithy" }))]
        );
        assert_eq!(
            calls_from_text(
                r#"{"calls":[{"tool":"go_to","args":{"place":"green"}},{"tool":"continue","args":{}}]}"#,
                &tools
            ),
            vec![call("go_to", json!({ "place": "green" })), call("continue", json!({}))]
        );
        assert_eq!(
            calls_from_text(r#"{"go_to": {"place": "chapel"}}"#, &tools),
            vec![call("go_to", json!({ "place": "chapel" }))]
        );
        assert_eq!(calls_from_text("I think I will just rest.", &tools), vec![]);
    }

    #[test]
    fn treats_null_like_a_missing_key() {
        let tools = tools();
        assert_eq!(
            calls_from_text(r#"{"name":null,"tool":"go_to","arguments":null,"args":{"place":"well"}}"#, &tools),
            vec![call("go_to", json!({ "place": "well" }))]
        );
        assert_eq!(calls_from_text(r#"{"name":5,"tool":"go_to"}"#, &tools), vec![]);
    }

    #[test]
    fn accepts_a_play_script_a_bare_tool_name_or_a_plain_spoken_line() {
        let conversation = tool("conversation", json!({ "lines": {}, "topic": {} }));
        let script = "Kael: Just the usual tug of the tide, Elowen.\n\nElowen: I see. Your answers are as elusive as your fish.\n\n**Kael**: \"And yet here I stand.\"";
        assert_eq!(
            calls_from_text(script, &[conversation]),
            vec![call(
                "conversation",
                json!({ "topic": "", "lines": [
                    { "speaker": "Kael", "text": "Just the usual tug of the tide, Elowen." },
                    { "speaker": "Elowen", "text": "I see. Your answers are as elusive as your fish." },
                    { "speaker": "Kael", "text": "And yet here I stand." },
                ] })
            )]
        );

        let react =
            vec![tool("fight", json!({ "say": {}, "thought": {} })), tool("flee", json!({ "say": {}, "thought": {} }))];
        assert_eq!(
            calls_from_text(
                "flee\nthought: I am too old for this.\nsay: The wind calls for rest.\nnote: ignored",
                &react
            ),
            vec![call("flee", json!({ "thought": "I am too old for this.", "say": "The wind calls for rest." }))]
        );

        let reply = tool("reply", json!({ "text": {}, "feeling": {}, "end": {} }));
        assert_eq!(
            calls_from_text("You always did have a sharp tongue.", &[reply]),
            vec![call(
                "reply",
                json!({ "text": "You always did have a sharp tongue.", "feeling": "same", "end": false })
            )]
        );
        assert_eq!(calls_from_text("You always did have a sharp tongue.", &tools()), vec![]);
    }

    #[test]
    fn bare_tool_lines_read_booleans() {
        let reply = tool("reply", json!({ "text": {}, "end": {} }));
        assert_eq!(
            calls_from_text("Reply\n\"end\" = \"true\",\ntext: 'Bye'\nend2: x", &[reply]),
            vec![call("reply", json!({ "end": true, "text": "Bye" }))]
        );
    }

    fn all_tools() -> Vec<ToolDef> {
        let mut all = tools();
        all.push(tool("conversation", json!({ "lines": {}, "topic": {} })));
        all.push(tool("flee", json!({ "say": {} })));
        all
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(2000))]

        #[test]
        fn never_panics_on_arbitrary_text(text in any::<String>()) {
            let _ = calls_from_text(&text, &all_tools());
            let _ = calls_from_text(&text, &[tool("reply", json!({ "text": {} }))]);
            let _ = loose_json_str(&text);
            let _ = strip_thinking(&text);
            for block in json_blocks(&text) {
                prop_assert!(text.contains(block));
            }
        }

        #[test]
        fn never_panics_on_json_like_text(text in r#"[\[\]{}"':,\\ a-z0-9<>/_\n]{0,200}"#) {
            let _ = calls_from_text(&text, &all_tools());
            let reply = message(json!({ "content": text, "tool_calls": [{ "function": { "name": "go_to", "arguments": text } }] }));
            let _ = calls_from_message(&reply, &all_tools());
        }
    }
}
