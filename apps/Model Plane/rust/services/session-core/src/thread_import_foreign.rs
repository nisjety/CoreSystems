//! The ChatGPT and Claude export readers of the thread import (Wave 3 EP-12 S15).
//!
//! A file in one of these formats becomes the [`ImportShape`] that the strict parse of a
//! `verevon.thread-export.v1` file builds. [`thread_import`] applies every rule after that: the
//! caps (D11), the role filter, the quoting that marks each turn untrusted, the `Importert:` title
//! and the `metadata.imported` marker. So a foreign turn is stored exactly as a verevon turn is.
//!
//! The converters are pure. They read a parsed value and return a shape. They do no I/O and never
//! touch the database. Only the text of `user` and `assistant` turns is kept, and everything else
//! is counted in `dropped`.
//!
//! * ChatGPT `conversations.json`. A conversation is an object with a `mapping` of nodes and a
//!   `current_node`. The active branch is the path from `current_node` up through each node's
//!   `parent` to the root, so a branch the user left behind is not read. The root has no message
//!   and is skipped. A message's text is its `content.parts` strings joined by a line break. Any
//!   other part (an image, a file, a tool result) is counted as dropped.
//! * Claude `conversations.json`. A conversation is an object with a `chat_messages` array. The
//!   sender `human` is the person and `assistant` is the model. A message's `text` is its text.
//!   Content blocks that are not text, and attachments and files, are counted as dropped.
//!
//! A conversation is the object itself, or the entry at `conversation_index` of an array. The
//! import reads entry 0 of an array: it creates one thread, and the ImportThread request has no
//! index field.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::thread_import::{ImportMessage, ImportShape, Invalid};

/// The foreign formats the import reads. The marker stores the name that [`ForeignSource::as_str`]
/// gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ForeignSource {
    /// A ChatGPT `conversations.json` conversation.
    ChatGpt,
    /// A Claude data-export `conversations.json` conversation.
    Claude,
}

impl ForeignSource {
    /// The name stored in `metadata.imported.source`.
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ChatGpt => "chatgpt",
            Self::Claude => "claude",
        }
    }
}

/// Why a foreign file is refused. A converter names one of the closed reasons the import already
/// returns: `shape` for a file that does not have the expected structure, and `empty` when an array
/// holds no conversation at the index. A refused foreign file therefore reads like any other.
pub(crate) type ImportError = Invalid;

/// The foreign format a parsed file is, judged by its first conversation. A ChatGPT conversation
/// has a `mapping` object and a Claude conversation has a `chat_messages` array. `None` for
/// anything else, which the import refuses as it refuses every unknown file.
pub(crate) fn detect(raw: &Value) -> Option<ForeignSource> {
    let conversation = pick_conversation(raw, 0).ok()?;
    if conversation.get("mapping").is_some_and(Value::is_object) {
        Some(ForeignSource::ChatGpt)
    } else if conversation
        .get("chat_messages")
        .is_some_and(Value::is_array)
    {
        Some(ForeignSource::Claude)
    } else {
        None
    }
}

/// Read one ChatGPT conversation. Its active branch is read oldest turn first. A `current_node`
/// that is not a string, a node that names no node of the mapping, a `parent` that is not a string,
/// and a loop are all shape errors, so the walk always ends.
pub(crate) fn from_chatgpt(
    raw: &Value,
    conversation_index: usize,
) -> Result<ImportShape, ImportError> {
    let conversation = pick_conversation(raw, conversation_index)?;
    let mapping = conversation
        .get("mapping")
        .and_then(Value::as_object)
        .ok_or(Invalid::Shape)?;
    let mut node_id = conversation
        .get("current_node")
        .and_then(Value::as_str)
        .ok_or(Invalid::Shape)?;
    let mut branch = Vec::new();
    let mut seen = HashSet::new();
    loop {
        if !seen.insert(node_id) {
            return Err(Invalid::Shape);
        }
        let node = mapping.get(node_id).ok_or(Invalid::Shape)?;
        branch.push(node);
        match node.get("parent") {
            None | Some(Value::Null) => break,
            Some(parent) => node_id = parent.as_str().ok_or(Invalid::Shape)?,
        }
    }
    let messages: Vec<ImportMessage> = branch
        .into_iter()
        .rev()
        .filter_map(|node| node.get("message").filter(|message| !message.is_null()))
        .map(chatgpt_message)
        .collect();
    Ok(ImportShape {
        title: text_at(conversation, "title"),
        exported_at: epoch_time_at(conversation, "update_time"),
        source_thread_id: String::new(),
        source: Some(ForeignSource::ChatGpt),
        messages,
    })
}

/// Read one Claude conversation: its messages in order, its name and its last update time.
pub(crate) fn from_claude(
    raw: &Value,
    conversation_index: usize,
) -> Result<ImportShape, ImportError> {
    let conversation = pick_conversation(raw, conversation_index)?;
    let chat_messages = conversation
        .get("chat_messages")
        .and_then(Value::as_array)
        .ok_or(Invalid::Shape)?;
    let messages = chat_messages
        .iter()
        .map(claude_message)
        .collect::<Result<Vec<ImportMessage>, ImportError>>()?;
    Ok(ImportShape {
        title: text_at(conversation, "name"),
        exported_at: rfc3339_at(conversation, "updated_at"),
        source_thread_id: String::new(),
        source: Some(ForeignSource::Claude),
        messages,
    })
}

/// One ChatGPT message. Its role is its author's role. Its text is its string parts joined by a
/// line break, and every part that is not a string is counted as dropped.
fn chatgpt_message(message: &Value) -> ImportMessage {
    let role = message
        .get("author")
        .and_then(|author| author.get("role"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    let parts = message
        .get("content")
        .and_then(|content| content.get("parts"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten();
    let mut texts = Vec::new();
    let mut dropped_extras = 0;
    for part in parts {
        match part.as_str() {
            Some(text) => texts.push(text),
            None => dropped_extras += 1,
        }
    }
    ImportMessage {
        role: role.to_owned(),
        content: texts.join("\n"),
        dropped_extras,
    }
}

/// One Claude message. `human` is the person and `assistant` is the model. Any other sender gets
/// no role, so the import drops the message. `text`, when present, must be a string. Content blocks
/// that are not text, and attachments and files, are counted and never stored.
fn claude_message(message: &Value) -> Result<ImportMessage, ImportError> {
    let role = match message.get("sender").and_then(Value::as_str) {
        Some("human") => "user",
        Some("assistant") => "assistant",
        _ => "",
    };
    let content = match message.get("text") {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => text.clone(),
        Some(_) => return Err(Invalid::Shape),
    };
    let other_blocks = message
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|block| block.get("type").and_then(Value::as_str) != Some("text"))
        .count();
    let attachments = ["attachments", "files"]
        .into_iter()
        .filter(|key| {
            message
                .get(*key)
                .and_then(Value::as_array)
                .is_some_and(|items| !items.is_empty())
        })
        .count();
    Ok(ImportMessage {
        role: role.to_owned(),
        content,
        dropped_extras: other_blocks + attachments,
    })
}

/// The conversation a file names: the object itself, or the entry at `index` of an array. An object
/// is one conversation, so only index 0 names it. An array with no entry at the index holds nothing
/// to import.
fn pick_conversation(raw: &Value, index: usize) -> Result<&Value, ImportError> {
    match raw {
        Value::Array(entries) => entries.get(index).ok_or(Invalid::Empty),
        Value::Object(_) if index == 0 => Ok(raw),
        Value::Object(_) => Err(Invalid::Empty),
        _ => Err(Invalid::Shape),
    }
}

/// A string field, or empty when it is absent, null or not a string. A title is cleaned where the
/// import reads it.
fn text_at(object: &Value, key: &str) -> String {
    object
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// A ChatGPT time: Unix seconds, possibly fractional, or null. `None` for anything that is not a
/// finite number, or that does not name an instant.
fn epoch_time_at(object: &Value, key: &str) -> Option<String> {
    let seconds = object.get(key)?.as_f64()?;
    if !seconds.is_finite() {
        return None;
    }
    // The float cast saturates, and a millisecond count out of range names no instant.
    DateTime::from_timestamp_millis((seconds * 1000.0).round() as i64).map(|time| time.to_rfc3339())
}

/// A time string in RFC 3339, normalised to UTC as the verevon export's time is. `None` when it is
/// absent or does not parse.
fn rfc3339_at(object: &Value, key: &str) -> Option<String> {
    let text = object.get(key)?.as_str()?;
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.with_timezone(&Utc).to_rfc3339())
}
