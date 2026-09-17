//! Privacy-preserving, versioned exports of the user's local conversation data.
//!
//! This module deliberately has no access to API profiles, settings, or the
//! credential store. The explicit projection below is an allow-list: adding a
//! new field to a database record cannot accidentally put secrets in an export.

use std::collections::HashSet;

use serde::Serialize;

use crate::{
    error::AppError,
    infrastructure::database::{ChatMessage, Database, MemoryRecord},
};

pub const EXPORT_FORMAT: &str = "nova-local-data";
pub const EXPORT_SCHEMA_VERSION: u32 = 1;

const MAX_MESSAGE_CHARS: usize = 200_000;
const MAX_MEMORY_CHARS: usize = 10_000;
const MAX_TIMESTAMP_CHARS: usize = 128;

/// The stable, portable JSON document produced by the local-data export.
///
/// It intentionally contains only chat messages and memories. In particular,
/// API endpoints, models, secret references, API keys, window data, and user
/// preferences are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalDataExport {
    format: &'static str,
    schema_version: u32,
    chat_messages: Vec<ExportedChatMessage>,
    memories: Vec<ExportedMemory>,
}

/// An explicitly allow-listed chat message for a portable export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedChatMessage {
    id: i64,
    role: String,
    content: String,
    created_at: String,
    status: String,
}

/// An explicitly allow-listed memory for a portable export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportedMemory {
    id: i64,
    content: String,
    source_message_id: i64,
    created_at: String,
    updated_at: String,
}

/// Loads, validates, and projects exportable data from the database.
///
/// The output order is always ascending by record ID, independent of database
/// query order, so equal input data produces identical JSON bytes.
pub fn build_local_data_export(database: &Database) -> Result<LocalDataExport, AppError> {
    let messages = database.list_chat_messages()?;
    let memories = database.list_memories()?;
    build_local_data_export_from_records(messages, memories)
}

/// Builds an export from records. This is public to keep the validation and
/// serialization behavior independently testable by a future command layer.
pub fn build_local_data_export_from_records(
    mut messages: Vec<ChatMessage>,
    mut memories: Vec<MemoryRecord>,
) -> Result<LocalDataExport, AppError> {
    messages.sort_by_key(|message| message.id);
    memories.sort_by_key(|memory| memory.id);

    let mut message_ids = HashSet::with_capacity(messages.len());
    let mut exported_messages = Vec::with_capacity(messages.len());
    for message in messages {
        validate_chat_message(&message, &mut message_ids)?;
        exported_messages.push(ExportedChatMessage {
            id: message.id,
            role: message.role,
            content: message.content,
            created_at: message.created_at,
            status: message.status,
        });
    }

    let mut memory_ids = HashSet::with_capacity(memories.len());
    let mut exported_memories = Vec::with_capacity(memories.len());
    for memory in memories {
        validate_memory(&memory, &message_ids, &mut memory_ids)?;
        exported_memories.push(ExportedMemory {
            id: memory.id,
            content: memory.content,
            source_message_id: memory.source_message_id,
            created_at: memory.created_at,
            updated_at: memory.updated_at,
        });
    }

    Ok(LocalDataExport {
        format: EXPORT_FORMAT,
        schema_version: EXPORT_SCHEMA_VERSION,
        chat_messages: exported_messages,
        memories: exported_memories,
    })
}

/// Serializes a stable, human-readable JSON export with a trailing newline.
pub fn local_data_export_json(database: &Database) -> Result<String, AppError> {
    let export = build_local_data_export(database)?;
    serialize_local_data_export(&export)
}

/// Serializes a previously validated export. Kept separate for command layers
/// that want to present a save dialog before writing to a user-selected path.
pub fn serialize_local_data_export(export: &LocalDataExport) -> Result<String, AppError> {
    let mut json = serde_json::to_string_pretty(export).map_err(|error| {
        AppError::internal(format!("failed to serialize local data export: {error}"))
    })?;
    json.push('\n');
    Ok(json)
}

fn validate_chat_message(
    message: &ChatMessage,
    message_ids: &mut HashSet<i64>,
) -> Result<(), AppError> {
    validate_positive_unique_id("chat message", message.id, message_ids)?;
    validate_nonempty_text("chat message role", &message.role, 32)?;
    if !matches!(message.role.as_str(), "user" | "assistant" | "system") {
        return Err(invalid_export_data("chat message has an unsupported role"));
    }
    validate_nonempty_text("chat message content", &message.content, MAX_MESSAGE_CHARS)?;
    validate_nonempty_text(
        "chat message creation time",
        &message.created_at,
        MAX_TIMESTAMP_CHARS,
    )?;
    validate_nonempty_text("chat message status", &message.status, 32)?;
    if !matches!(message.status.as_str(), "pending" | "sent" | "failed") {
        return Err(invalid_export_data(
            "chat message has an unsupported status",
        ));
    }
    Ok(())
}

fn validate_memory(
    memory: &MemoryRecord,
    message_ids: &HashSet<i64>,
    memory_ids: &mut HashSet<i64>,
) -> Result<(), AppError> {
    validate_positive_unique_id("memory", memory.id, memory_ids)?;
    validate_nonempty_text("memory content", &memory.content, MAX_MEMORY_CHARS)?;
    validate_nonempty_text(
        "memory creation time",
        &memory.created_at,
        MAX_TIMESTAMP_CHARS,
    )?;
    validate_nonempty_text(
        "memory update time",
        &memory.updated_at,
        MAX_TIMESTAMP_CHARS,
    )?;
    if !message_ids.contains(&memory.source_message_id) {
        return Err(invalid_export_data(
            "memory references a chat message that is not included in the export",
        ));
    }
    Ok(())
}

fn validate_positive_unique_id(
    label: &str,
    id: i64,
    ids: &mut HashSet<i64>,
) -> Result<(), AppError> {
    if id <= 0 {
        return Err(invalid_export_data(format!("{label} has an invalid ID")));
    }
    if !ids.insert(id) {
        return Err(invalid_export_data(format!("duplicate {label} ID")));
    }
    Ok(())
}

fn validate_nonempty_text(label: &str, value: &str, max_chars: usize) -> Result<(), AppError> {
    if value.trim().is_empty() {
        return Err(invalid_export_data(format!("{label} is empty")));
    }
    if value.chars().count() > max_chars {
        return Err(invalid_export_data(format!(
            "{label} exceeds the export limit"
        )));
    }
    Ok(())
}

fn invalid_export_data(message: impl Into<String>) -> AppError {
    AppError::database(format!(
        "cannot export invalid local data: {}",
        message.into()
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        EXPORT_FORMAT, EXPORT_SCHEMA_VERSION, build_local_data_export_from_records,
        serialize_local_data_export,
    };
    use crate::infrastructure::database::{ChatMessage, MemoryRecord};

    fn message(id: i64, role: &str, status: &str) -> ChatMessage {
        ChatMessage {
            id,
            role: role.to_owned(),
            content: format!("message {id}"),
            created_at: "2026-09-17 12:00:00".to_owned(),
            status: status.to_owned(),
        }
    }

    fn memory(id: i64, source_message_id: i64) -> MemoryRecord {
        MemoryRecord {
            id,
            content: format!("memory {id}"),
            source_message_id,
            created_at: "2026-09-17 12:00:00".to_owned(),
            updated_at: "2026-09-17 12:01:00".to_owned(),
        }
    }

    #[test]
    fn export_is_deterministic_and_contains_only_allowlisted_fields() {
        let export = build_local_data_export_from_records(
            vec![
                message(2, "assistant", "sent"),
                message(1, "user", "pending"),
            ],
            vec![memory(2, 2), memory(1, 1)],
        )
        .expect("valid records export");

        let json = serialize_local_data_export(&export).expect("serialize export");
        assert_eq!(export.format, EXPORT_FORMAT);
        assert_eq!(export.schema_version, EXPORT_SCHEMA_VERSION);
        assert_eq!(
            json,
            concat!(
                "{\n",
                "  \"format\": \"nova-local-data\",\n",
                "  \"schemaVersion\": 1,\n",
                "  \"chatMessages\": [\n",
                "    {\n",
                "      \"id\": 1,\n",
                "      \"role\": \"user\",\n",
                "      \"content\": \"message 1\",\n",
                "      \"createdAt\": \"2026-09-17 12:00:00\",\n",
                "      \"status\": \"pending\"\n",
                "    },\n",
                "    {\n",
                "      \"id\": 2,\n",
                "      \"role\": \"assistant\",\n",
                "      \"content\": \"message 2\",\n",
                "      \"createdAt\": \"2026-09-17 12:00:00\",\n",
                "      \"status\": \"sent\"\n",
                "    }\n",
                "  ],\n",
                "  \"memories\": [\n",
                "    {\n",
                "      \"id\": 1,\n",
                "      \"content\": \"memory 1\",\n",
                "      \"sourceMessageId\": 1,\n",
                "      \"createdAt\": \"2026-09-17 12:00:00\",\n",
                "      \"updatedAt\": \"2026-09-17 12:01:00\"\n",
                "    },\n",
                "    {\n",
                "      \"id\": 2,\n",
                "      \"content\": \"memory 2\",\n",
                "      \"sourceMessageId\": 2,\n",
                "      \"createdAt\": \"2026-09-17 12:00:00\",\n",
                "      \"updatedAt\": \"2026-09-17 12:01:00\"\n",
                "    }\n",
                "  ]\n",
                "}\n"
            )
        );
        for forbidden in [
            "apiKey",
            "secretRef",
            "baseUrl",
            "apiProfiles",
            "settings",
            "windowState",
        ] {
            assert!(!json.contains(forbidden), "export leaked {forbidden}");
        }
    }

    #[test]
    fn invalid_records_are_mapped_to_database_errors() {
        let error =
            build_local_data_export_from_records(vec![message(1, "tool", "sent")], Vec::new())
                .expect_err("unsupported roles must not be exported");

        assert!(matches!(error, crate::error::AppError::Database(_)));
        assert!(error.to_string().contains("unsupported role"));
    }

    #[test]
    fn orphaned_memories_are_rejected() {
        let error = build_local_data_export_from_records(
            vec![message(1, "user", "sent")],
            vec![memory(1, 99)],
        )
        .expect_err("memory source must be exported too");

        assert!(error.to_string().contains("not included"));
    }
}
