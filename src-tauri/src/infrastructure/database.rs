use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

const DATABASE_FILE_NAME: &str = "nova.db";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PersonaProfile {
    pub name: String,
    pub personality: String,
    #[serde(default, skip_serializing)]
    pub speech_style: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    pub theme: String,
    pub dark_mode: bool,
    pub dnd_start: Option<String>,
    pub dnd_end: Option<String>,
    pub voice_autoplay: bool,
    pub proactive_enabled: bool,
    pub tts_voice: String,
    pub tts_rate: i32,
    pub tts_pitch: i32,
    pub tts_volume: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowState {
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub x: Option<i32>,
    pub y: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiProfileRecord {
    pub capability: String,
    pub base_url: String,
    pub path: String,
    pub model: String,
    pub secret_ref: String,
    pub enabled: bool,
    pub last_tested_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub created_at: String,
    pub status: String,
}

/// A durable user fact derived from one message in the local conversation timeline.
///
/// The source relation is intentionally mandatory: automatic memory extraction must
/// always leave the user a way to inspect the message that produced a memory.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryRecord {
    pub id: i64,
    pub content: String,
    pub source_message_id: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// An application-local calendar item. Datetimes are stored as ISO-8601 text so
/// the command layer can preserve the user's timezone offset without conversion.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRecord {
    pub id: i64,
    pub title: String,
    pub scheduled_at: String,
    pub remind_at: String,
    pub source_message_id: Option<i64>,
    pub status: String,
}

pub struct Database {
    connection: Mutex<Connection>,
    path: PathBuf,
}

impl Database {
    pub fn initialize(app_data_dir: &Path) -> Result<Self, AppError> {
        fs::create_dir_all(app_data_dir).map_err(|error| {
            AppError::database(format!(
                "failed to create the application data directory: {error}"
            ))
        })?;

        let path = app_data_dir.join(DATABASE_FILE_NAME);
        let mut connection = Connection::open(&path)
            .map_err(|error| AppError::database(format!("failed to open the database: {error}")))?;
        connection
            .pragma_update(None, "foreign_keys", "ON")
            .map_err(|error| {
                AppError::database(format!("failed to enable foreign keys: {error}"))
            })?;
        migrate(&mut connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
            path,
        })
    }

    pub fn is_ready(&self) -> bool {
        self.connection.lock().is_ok() && self.path.exists()
    }

    pub fn onboarding_required(&self) -> Result<bool, AppError> {
        self.connection()?
            .query_row(
                "SELECT onboarding_required FROM app_settings WHERE id = 1",
                [],
                |row| Ok(row.get::<_, i64>(0)? != 0),
            )
            .map_err(|error| {
                AppError::database(format!("failed to read onboarding state: {error}"))
            })
    }

    pub fn set_onboarding_required(&self, required: bool) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET onboarding_required = ?1 WHERE id = 1",
                [i64::from(required)],
            )
            .map_err(|error| {
                AppError::database(format!("failed to save onboarding state: {error}"))
            })?;
        Ok(())
    }

    pub fn get_persona(&self) -> Result<Option<PersonaProfile>, AppError> {
        self.connection()?
            .query_row(
                "SELECT name, personality, speech_style FROM persona_profile WHERE id = 1",
                [],
                |row| {
                    Ok(PersonaProfile {
                        name: row.get(0)?,
                        personality: row.get(1)?,
                        speech_style: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(|error| AppError::database(format!("failed to read persona: {error}")))
    }

    pub fn save_persona(&self, persona: &PersonaProfile) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "INSERT INTO persona_profile (id, name, personality, speech_style, updated_at)
                 VALUES (1, ?1, ?2, ?3, CURRENT_TIMESTAMP)
                 ON CONFLICT(id) DO UPDATE SET
                    name = excluded.name,
                    personality = excluded.personality,
                    speech_style = excluded.speech_style,
                    updated_at = CURRENT_TIMESTAMP",
                params![persona.name, persona.personality, persona.speech_style],
            )
            .map_err(|error| AppError::database(format!("failed to save persona: {error}")))?;
        Ok(())
    }

    pub fn get_settings(&self) -> Result<AppSettings, AppError> {
        self.connection()?
            .query_row(
                "SELECT theme, dark_mode, dnd_start, dnd_end, voice_autoplay, proactive_enabled,
                        tts_voice, tts_rate, tts_pitch, tts_volume
                 FROM app_settings WHERE id = 1",
                [],
                |row| {
                    Ok(AppSettings {
                        theme: row.get(0)?,
                        dark_mode: row.get::<_, i64>(1)? != 0,
                        dnd_start: row.get(2)?,
                        dnd_end: row.get(3)?,
                        voice_autoplay: row.get::<_, i64>(4)? != 0,
                        proactive_enabled: row.get::<_, i64>(5)? != 0,
                        tts_voice: row.get(6)?,
                        tts_rate: row.get(7)?,
                        tts_pitch: row.get(8)?,
                        tts_volume: row.get(9)?,
                    })
                },
            )
            .map_err(|error| AppError::database(format!("failed to read settings: {error}")))
    }

    pub fn save_settings(&self, settings: &AppSettings) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings SET
                    theme = ?1,
                    dark_mode = ?2,
                    dnd_start = ?3,
                    dnd_end = ?4,
                    voice_autoplay = ?5,
                    proactive_enabled = ?6,
                    tts_voice = ?7,
                    tts_rate = ?8,
                    tts_pitch = ?9,
                    tts_volume = ?10
                 WHERE id = 1",
                params![
                    settings.theme,
                    settings.dark_mode,
                    settings.dnd_start,
                    settings.dnd_end,
                    settings.voice_autoplay,
                    settings.proactive_enabled,
                    settings.tts_voice,
                    settings.tts_rate,
                    settings.tts_pitch,
                    settings.tts_volume,
                ],
            )
            .map_err(|error| AppError::database(format!("failed to save settings: {error}")))?;
        Ok(())
    }

    pub fn get_window_state(&self) -> Result<WindowState, AppError> {
        self.connection()?
            .query_row(
                "SELECT window_mode, window_width, window_height, window_x, window_y
                 FROM app_settings WHERE id = 1",
                [],
                |row| {
                    Ok(WindowState {
                        mode: row.get(0)?,
                        width: row.get(1)?,
                        height: row.get(2)?,
                        x: row.get(3)?,
                        y: row.get(4)?,
                    })
                },
            )
            .map_err(|error| AppError::database(format!("failed to read window state: {error}")))
    }

    pub fn save_window_mode(&self, mode: &str, width: u32, height: u32) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings
                 SET window_mode = ?1, window_width = ?2, window_height = ?3
                 WHERE id = 1",
                params![mode, width, height],
            )
            .map_err(|error| AppError::database(format!("failed to save window mode: {error}")))?;
        Ok(())
    }

    pub fn save_window_geometry(
        &self,
        width: u32,
        height: u32,
        x: i32,
        y: i32,
    ) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE app_settings
                 SET window_width = ?1, window_height = ?2, window_x = ?3, window_y = ?4
                 WHERE id = 1",
                params![width, height, x, y],
            )
            .map_err(|error| {
                AppError::database(format!("failed to save window geometry: {error}"))
            })?;
        Ok(())
    }

    pub fn get_api_profile(&self, capability: &str) -> Result<Option<ApiProfileRecord>, AppError> {
        self.connection()?
            .query_row(
                "SELECT capability, base_url, path, model, secret_ref, enabled, last_tested_at
                 FROM api_profiles WHERE capability = ?1",
                [capability],
                |row| {
                    Ok(ApiProfileRecord {
                        capability: row.get(0)?,
                        base_url: row.get(1)?,
                        path: row.get(2)?,
                        model: row.get(3)?,
                        secret_ref: row.get(4)?,
                        enabled: row.get::<_, i64>(5)? != 0,
                        last_tested_at: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|error| AppError::database(format!("failed to read API profile: {error}")))
    }

    pub fn list_api_secret_refs(&self) -> Result<Vec<String>, AppError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare("SELECT secret_ref FROM api_profiles")
            .map_err(|error| {
                AppError::database(format!("failed to prepare API secret query: {error}"))
            })?;
        statement
            .query_map([], |row| row.get(0))
            .map_err(|error| AppError::database(format!("failed to read API secrets: {error}")))?
            .collect::<Result<Vec<String>, _>>()
            .map_err(|error| AppError::database(format!("failed to decode API secrets: {error}")))
    }

    pub fn save_api_profile(&self, profile: &ApiProfileRecord) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "INSERT INTO api_profiles
                    (capability, base_url, path, model, secret_ref, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(capability) DO UPDATE SET
                    base_url = excluded.base_url,
                    path = excluded.path,
                    model = excluded.model,
                    secret_ref = excluded.secret_ref,
                    enabled = excluded.enabled,
                    last_tested_at = NULL",
                params![
                    profile.capability,
                    profile.base_url,
                    profile.path,
                    profile.model,
                    profile.secret_ref,
                    profile.enabled,
                ],
            )
            .map_err(|error| AppError::database(format!("failed to save API profile: {error}")))?;
        Ok(())
    }

    pub fn mark_api_profile_tested(&self, capability: &str) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE api_profiles SET last_tested_at = CURRENT_TIMESTAMP WHERE capability = ?1",
                [capability],
            )
            .map_err(|error| {
                AppError::database(format!("failed to record API connection test: {error}"))
            })?;
        Ok(())
    }

    pub fn list_chat_messages(&self) -> Result<Vec<ChatMessage>, AppError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, role, content, created_at, status
                 FROM chat_messages ORDER BY id ASC",
            )
            .map_err(|error| {
                AppError::database(format!("failed to prepare chat query: {error}"))
            })?;
        let messages = statement
            .query_map([], |row| {
                Ok(ChatMessage {
                    id: row.get(0)?,
                    role: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                    status: row.get(4)?,
                })
            })
            .map_err(|error| AppError::database(format!("failed to read chat messages: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| {
                AppError::database(format!("failed to decode chat messages: {error}"))
            })?;
        Ok(messages)
    }

    pub fn get_chat_message(&self, id: i64) -> Result<Option<ChatMessage>, AppError> {
        self.connection()?
            .query_row(
                "SELECT id, role, content, created_at, status FROM chat_messages WHERE id = ?1",
                [id],
                |row| {
                    Ok(ChatMessage {
                        id: row.get(0)?,
                        role: row.get(1)?,
                        content: row.get(2)?,
                        created_at: row.get(3)?,
                        status: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(|error| AppError::database(format!("failed to read chat message: {error}")))
    }

    pub fn insert_chat_message(
        &self,
        role: &str,
        content: &str,
        status: &str,
    ) -> Result<ChatMessage, AppError> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO chat_messages (role, content, status) VALUES (?1, ?2, ?3)",
                params![role, content, status],
            )
            .map_err(|error| AppError::database(format!("failed to save chat message: {error}")))?;
        let id = connection.last_insert_rowid();
        connection
            .query_row(
                "SELECT id, role, content, created_at, status FROM chat_messages WHERE id = ?1",
                [id],
                |row| {
                    Ok(ChatMessage {
                        id: row.get(0)?,
                        role: row.get(1)?,
                        content: row.get(2)?,
                        created_at: row.get(3)?,
                        status: row.get(4)?,
                    })
                },
            )
            .map_err(|error| AppError::database(format!("failed to reload chat message: {error}")))
    }

    pub fn update_chat_message_status(&self, id: i64, status: &str) -> Result<(), AppError> {
        self.connection()?
            .execute(
                "UPDATE chat_messages SET status = ?1 WHERE id = ?2",
                params![status, id],
            )
            .map_err(|error| {
                AppError::database(format!("failed to update chat message status: {error}"))
            })?;
        Ok(())
    }

    pub fn list_memories(&self) -> Result<Vec<MemoryRecord>, AppError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, content, source_message_id, created_at, updated_at
                 FROM memories ORDER BY updated_at DESC, id DESC",
            )
            .map_err(|error| {
                AppError::database(format!("failed to prepare memory query: {error}"))
            })?;
        statement
            .query_map([], memory_from_row)
            .map_err(|error| AppError::database(format!("failed to read memories: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::database(format!("failed to decode memories: {error}")))
    }

    pub fn get_memory(&self, id: i64) -> Result<Option<MemoryRecord>, AppError> {
        self.connection()?
            .query_row(
                "SELECT id, content, source_message_id, created_at, updated_at
                 FROM memories WHERE id = ?1",
                [id],
                memory_from_row,
            )
            .optional()
            .map_err(|error| AppError::database(format!("failed to read memory: {error}")))
    }

    pub fn insert_memory(
        &self,
        content: &str,
        source_message_id: i64,
    ) -> Result<MemoryRecord, AppError> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO memories (content, source_message_id) VALUES (?1, ?2)",
                params![content, source_message_id],
            )
            .map_err(|error| AppError::database(format!("failed to save memory: {error}")))?;
        get_memory_from_connection(&connection, connection.last_insert_rowid())?
            .ok_or_else(|| AppError::database("saved memory could not be reloaded"))
    }

    /// Updates a memory's user-editable text and returns `None` when it no longer exists.
    pub fn update_memory(&self, id: i64, content: &str) -> Result<Option<MemoryRecord>, AppError> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE memories SET content = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
                params![content, id],
            )
            .map_err(|error| AppError::database(format!("failed to update memory: {error}")))?;
        if changed == 0 {
            return Ok(None);
        }
        get_memory_from_connection(&connection, id)
    }

    /// Deletes a memory and reports whether an existing row was removed.
    pub fn delete_memory(&self, id: i64) -> Result<bool, AppError> {
        self.connection()?
            .execute("DELETE FROM memories WHERE id = ?1", [id])
            .map(|changed| changed != 0)
            .map_err(|error| AppError::database(format!("failed to delete memory: {error}")))
    }

    pub fn list_schedules(&self) -> Result<Vec<ScheduleRecord>, AppError> {
        let connection = self.connection()?;
        let mut statement = connection
            .prepare(
                "SELECT id, title, scheduled_at, remind_at, source_message_id, status
                 FROM schedules ORDER BY scheduled_at ASC, id ASC",
            )
            .map_err(|error| {
                AppError::database(format!("failed to prepare schedule query: {error}"))
            })?;
        statement
            .query_map([], schedule_from_row)
            .map_err(|error| AppError::database(format!("failed to read schedules: {error}")))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| AppError::database(format!("failed to decode schedules: {error}")))
    }

    pub fn get_schedule(&self, id: i64) -> Result<Option<ScheduleRecord>, AppError> {
        self.connection()?
            .query_row(
                "SELECT id, title, scheduled_at, remind_at, source_message_id, status
                 FROM schedules WHERE id = ?1",
                [id],
                schedule_from_row,
            )
            .optional()
            .map_err(|error| AppError::database(format!("failed to read schedule: {error}")))
    }

    /// Persists a schedule only after the conversation confirmation flow approves it.
    pub fn insert_schedule(
        &self,
        title: &str,
        scheduled_at: &str,
        remind_at: &str,
        source_message_id: Option<i64>,
        status: &str,
    ) -> Result<ScheduleRecord, AppError> {
        let connection = self.connection()?;
        connection
            .execute(
                "INSERT INTO schedules (title, scheduled_at, remind_at, source_message_id, status)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![title, scheduled_at, remind_at, source_message_id, status],
            )
            .map_err(|error| AppError::database(format!("failed to save schedule: {error}")))?;
        get_schedule_from_connection(&connection, connection.last_insert_rowid())?
            .ok_or_else(|| AppError::database("saved schedule could not be reloaded"))
    }

    /// Updates all mutable schedule fields and returns `None` when the schedule is absent.
    pub fn update_schedule(
        &self,
        id: i64,
        title: &str,
        scheduled_at: &str,
        remind_at: &str,
        status: &str,
    ) -> Result<Option<ScheduleRecord>, AppError> {
        let connection = self.connection()?;
        let changed = connection
            .execute(
                "UPDATE schedules
                 SET title = ?1, scheduled_at = ?2, remind_at = ?3, status = ?4
                 WHERE id = ?5",
                params![title, scheduled_at, remind_at, status, id],
            )
            .map_err(|error| AppError::database(format!("failed to update schedule: {error}")))?;
        if changed == 0 {
            return Ok(None);
        }
        get_schedule_from_connection(&connection, id)
    }

    /// Deletes an application-local schedule and reports whether it existed.
    pub fn delete_schedule(&self, id: i64) -> Result<bool, AppError> {
        self.connection()?
            .execute("DELETE FROM schedules WHERE id = ?1", [id])
            .map(|changed| changed != 0)
            .map_err(|error| AppError::database(format!("failed to delete schedule: {error}")))
    }

    pub fn clear_conversation_records(&self) -> Result<(), AppError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!(
                "failed to start clearing conversation data: {error}"
            ))
        })?;
        transaction
            .execute_batch(
                "DELETE FROM proactive_events;
                 DELETE FROM memories;
                 DELETE FROM chat_messages;
                 DELETE FROM sqlite_sequence
                 WHERE name IN ('proactive_events', 'memories', 'chat_messages');",
            )
            .map_err(|error| {
                AppError::database(format!("failed to clear conversation data: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!(
                "failed to commit clearing conversation data: {error}"
            ))
        })?;
        Ok(())
    }

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.connection
            .lock()
            .map_err(|_| AppError::database("database connection lock was poisoned"))
    }
}

fn memory_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MemoryRecord> {
    Ok(MemoryRecord {
        id: row.get(0)?,
        content: row.get(1)?,
        source_message_id: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
    })
}

fn get_memory_from_connection(
    connection: &Connection,
    id: i64,
) -> Result<Option<MemoryRecord>, AppError> {
    connection
        .query_row(
            "SELECT id, content, source_message_id, created_at, updated_at
             FROM memories WHERE id = ?1",
            [id],
            memory_from_row,
        )
        .optional()
        .map_err(|error| AppError::database(format!("failed to reload memory: {error}")))
}

fn schedule_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ScheduleRecord> {
    Ok(ScheduleRecord {
        id: row.get(0)?,
        title: row.get(1)?,
        scheduled_at: row.get(2)?,
        remind_at: row.get(3)?,
        source_message_id: row.get(4)?,
        status: row.get(5)?,
    })
}

fn get_schedule_from_connection(
    connection: &Connection,
    id: i64,
) -> Result<Option<ScheduleRecord>, AppError> {
    connection
        .query_row(
            "SELECT id, title, scheduled_at, remind_at, source_message_id, status
             FROM schedules WHERE id = ?1",
            [id],
            schedule_from_row,
        )
        .optional()
        .map_err(|error| AppError::database(format!("failed to reload schedule: {error}")))
}

fn migrate(connection: &mut Connection) -> Result<(), AppError> {
    connection
        .execute_batch(
            "
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS persona_profile (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                name TEXT NOT NULL,
                personality TEXT NOT NULL,
                speech_style TEXT NOT NULL,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS api_profiles (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                capability TEXT NOT NULL UNIQUE,
                base_url TEXT NOT NULL,
                path TEXT NOT NULL,
                model TEXT NOT NULL,
                secret_ref TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
            );

            CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                audio_ref TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                source_message_id INTEGER NOT NULL REFERENCES chat_messages(id) ON DELETE CASCADE,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                scheduled_at TEXT NOT NULL,
                remind_at TEXT NOT NULL,
                source_message_id INTEGER REFERENCES chat_messages(id) ON DELETE SET NULL,
                status TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS proactive_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL UNIQUE REFERENCES chat_messages(id) ON DELETE CASCADE,
                idle_started_at TEXT NOT NULL,
                notified_at TEXT,
                opened_at TEXT
            );

            CREATE TABLE IF NOT EXISTS app_settings (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                theme TEXT NOT NULL DEFAULT 'rose',
                dark_mode INTEGER NOT NULL DEFAULT 0 CHECK (dark_mode IN (0, 1)),
                dnd_start TEXT,
                dnd_end TEXT,
                voice_autoplay INTEGER NOT NULL DEFAULT 1 CHECK (voice_autoplay IN (0, 1))
            );

            INSERT OR IGNORE INTO app_settings (id) VALUES (1);
            INSERT OR IGNORE INTO schema_migrations (version) VALUES (1);
            ",
        )
        .map_err(|error| AppError::database(format!("failed to migrate the database: {error}")))?;

    let current_version = connection
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get::<_, Option<i64>>(0)
        })
        .map_err(|error| AppError::database(format!("failed to read the schema version: {error}")))?
        .unwrap_or_default();

    if current_version < 2 {
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!("failed to start database migration 2: {error}"))
        })?;
        transaction
            .execute_batch(
                "ALTER TABLE app_settings ADD COLUMN proactive_enabled INTEGER NOT NULL DEFAULT 1
                    CHECK (proactive_enabled IN (0, 1));
                 ALTER TABLE app_settings ADD COLUMN window_mode TEXT NOT NULL DEFAULT 'management';
                 ALTER TABLE app_settings ADD COLUMN window_width INTEGER NOT NULL DEFAULT 1080;
                 ALTER TABLE app_settings ADD COLUMN window_height INTEGER NOT NULL DEFAULT 760;
                 ALTER TABLE app_settings ADD COLUMN window_x INTEGER;
                 ALTER TABLE app_settings ADD COLUMN window_y INTEGER;
                 INSERT INTO schema_migrations (version) VALUES (2);",
            )
            .map_err(|error| {
                AppError::database(format!("failed to apply database migration 2: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!("failed to commit database migration 2: {error}"))
        })?;
    }

    if current_version < 3 {
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!("failed to start database migration 3: {error}"))
        })?;
        transaction
            .execute_batch(
                "ALTER TABLE api_profiles ADD COLUMN last_tested_at TEXT;
                 INSERT INTO schema_migrations (version) VALUES (3);",
            )
            .map_err(|error| {
                AppError::database(format!("failed to apply database migration 3: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!("failed to commit database migration 3: {error}"))
        })?;
    }

    if current_version < 4 {
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!("failed to start database migration 4: {error}"))
        })?;
        transaction
            .execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_memories_source_message
                    ON memories(source_message_id);
                 CREATE INDEX IF NOT EXISTS idx_schedules_scheduled_at
                    ON schedules(scheduled_at);
                 CREATE INDEX IF NOT EXISTS idx_schedules_source_message
                    ON schedules(source_message_id);
                 INSERT INTO schema_migrations (version) VALUES (4);",
            )
            .map_err(|error| {
                AppError::database(format!("failed to apply database migration 4: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!("failed to commit database migration 4: {error}"))
        })?;
    }

    if current_version < 5 {
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!("failed to start database migration 5: {error}"))
        })?;
        transaction
            .execute_batch(
                "ALTER TABLE app_settings ADD COLUMN tts_voice TEXT NOT NULL DEFAULT 'zh-CN-XiaoxiaoNeural';
                 ALTER TABLE app_settings ADD COLUMN tts_rate INTEGER NOT NULL DEFAULT -5;
                 ALTER TABLE app_settings ADD COLUMN tts_pitch INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE app_settings ADD COLUMN tts_volume INTEGER NOT NULL DEFAULT 0;
                 INSERT INTO schema_migrations (version) VALUES (5);",
            )
            .map_err(|error| {
                AppError::database(format!("failed to apply database migration 5: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!("failed to commit database migration 5: {error}"))
        })?;
    }

    if current_version < 6 {
        let transaction = connection.transaction().map_err(|error| {
            AppError::database(format!("failed to start database migration 6: {error}"))
        })?;
        transaction
            .execute_batch(
                "ALTER TABLE app_settings ADD COLUMN onboarding_required INTEGER NOT NULL DEFAULT 0
                    CHECK (onboarding_required IN (0, 1));
                 INSERT INTO schema_migrations (version) VALUES (6);",
            )
            .map_err(|error| {
                AppError::database(format!("failed to apply database migration 6: {error}"))
            })?;
        transaction.commit().map_err(|error| {
            AppError::database(format!("failed to commit database migration 6: {error}"))
        })?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use tempfile::tempdir;

    use rusqlite::Connection;

    use super::{ApiProfileRecord, AppSettings, Database, PersonaProfile};

    #[test]
    fn creates_a_database_and_reopens_it() {
        let directory = tempdir().expect("temporary directory");

        let database = Database::initialize(directory.path()).expect("database should initialize");
        assert!(database.is_ready());
        drop(database);

        let reopened = Database::initialize(directory.path()).expect("database should reopen");
        assert!(reopened.is_ready());
    }

    #[test]
    fn database_uses_the_standard_sqlite_file_header() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        drop(database);
        let bytes = std::fs::read(directory.path().join("nova.db")).expect("database file");

        assert_eq!(&bytes[..16], b"SQLite format 3\0");
    }

    #[test]
    fn database_file_name_is_scoped_to_app_data_directory() {
        let expected = Path::new("somewhere").join("nova.db");
        assert_eq!(
            expected.file_name().and_then(|name| name.to_str()),
            Some("nova.db")
        );
    }

    #[test]
    fn saves_and_reads_persona_and_settings() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let persona = PersonaProfile {
            name: "小诺".to_owned(),
            personality: "温柔、爱倾听".to_owned(),
            speech_style: "自然、轻松".to_owned(),
        };
        database.save_persona(&persona).expect("save persona");
        assert_eq!(database.get_persona().expect("read persona"), Some(persona));

        let settings = AppSettings {
            theme: "lavender".to_owned(),
            dark_mode: true,
            dnd_start: Some("23:00".to_owned()),
            dnd_end: Some("08:00".to_owned()),
            voice_autoplay: false,
            proactive_enabled: false,
            tts_voice: "zh-CN-XiaoxiaoNeural".to_owned(),
            tts_rate: -5,
            tts_pitch: 0,
            tts_volume: 0,
        };
        database.save_settings(&settings).expect("save settings");
        assert_eq!(database.get_settings().expect("read settings"), settings);
    }

    #[test]
    fn clears_only_memories_and_conversation_records() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        database
            .save_persona(&PersonaProfile {
                name: "小诺".to_owned(),
                personality: "温柔".to_owned(),
                speech_style: "温柔".to_owned(),
            })
            .expect("save persona");
        database
            .save_api_profile(&ApiProfileRecord {
                capability: "chat".to_owned(),
                base_url: "https://api.example.com/v1".to_owned(),
                path: "/chat/completions".to_owned(),
                model: "gpt-4.1-mini".to_owned(),
                secret_ref: "api-chat".to_owned(),
                enabled: true,
                last_tested_at: None,
            })
            .expect("save API profile");
        let settings = AppSettings {
            theme: "mint".to_owned(),
            dark_mode: true,
            dnd_start: Some("22:00".to_owned()),
            dnd_end: Some("07:00".to_owned()),
            voice_autoplay: false,
            proactive_enabled: false,
            tts_voice: "zh-CN-XiaoxiaoNeural".to_owned(),
            tts_rate: -5,
            tts_pitch: 0,
            tts_volume: 0,
        };
        database.save_settings(&settings).expect("save settings");
        let source = database
            .insert_chat_message("user", "记住我喜欢散步", "sent")
            .expect("save source message");
        database
            .insert_memory("我喜欢散步", source.id)
            .expect("save memory");
        database
            .insert_schedule(
                "散步",
                "2026-09-18T18:00",
                "2026-09-18T17:50",
                Some(source.id),
                "scheduled",
            )
            .expect("save schedule");

        database
            .clear_conversation_records()
            .expect("clear conversation records");

        assert!(
            !database
                .onboarding_required()
                .expect("read unchanged onboarding state")
        );
        assert!(database.get_persona().expect("read persona").is_some());
        assert_eq!(
            database.get_api_profile("chat").expect("read API profile"),
            Some(ApiProfileRecord {
                capability: "chat".to_owned(),
                base_url: "https://api.example.com/v1".to_owned(),
                path: "/chat/completions".to_owned(),
                model: "gpt-4.1-mini".to_owned(),
                secret_ref: "api-chat".to_owned(),
                enabled: true,
                last_tested_at: None,
            })
        );
        assert!(
            database
                .list_chat_messages()
                .expect("list messages")
                .is_empty()
        );
        assert!(database.list_memories().expect("list memories").is_empty());
        let schedules = database.list_schedules().expect("list schedules");
        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].source_message_id, None);
        assert_eq!(database.get_settings().expect("read settings"), settings);
    }

    #[test]
    fn migrates_an_existing_version_one_database() {
        let directory = tempdir().expect("temporary directory");
        let path = directory.path().join("nova.db");
        let connection = Connection::open(&path).expect("open old database");
        connection
            .execute_batch(
                "CREATE TABLE schema_migrations (
                    version INTEGER PRIMARY KEY,
                    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                CREATE TABLE app_settings (
                    id INTEGER PRIMARY KEY CHECK (id = 1),
                    theme TEXT NOT NULL DEFAULT 'rose',
                    dark_mode INTEGER NOT NULL DEFAULT 0 CHECK (dark_mode IN (0, 1)),
                    dnd_start TEXT,
                    dnd_end TEXT,
                    voice_autoplay INTEGER NOT NULL DEFAULT 1 CHECK (voice_autoplay IN (0, 1))
                );
                INSERT INTO app_settings (id) VALUES (1);
                INSERT INTO schema_migrations (version) VALUES (1);",
            )
            .expect("create old schema");
        drop(connection);

        let database = Database::initialize(directory.path()).expect("migrate old database");
        let state = database.get_window_state().expect("read migrated state");
        assert_eq!(state.mode, "management");
        assert_eq!((state.width, state.height), (1080, 760));
    }

    #[test]
    fn saves_api_profile_without_storing_the_secret() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let profile = ApiProfileRecord {
            capability: "chat".to_owned(),
            base_url: "https://api.example.com/v1".to_owned(),
            path: "/chat/completions".to_owned(),
            model: "gpt-4.1-mini".to_owned(),
            secret_ref: "api-chat".to_owned(),
            enabled: true,
            last_tested_at: None,
        };

        database.save_api_profile(&profile).expect("save profile");
        assert_eq!(
            database.get_api_profile("chat").expect("read profile"),
            Some(profile)
        );
        let bytes = std::fs::read(directory.path().join("nova.db")).expect("database file");
        assert!(!bytes.windows(10).any(|window| window == b"secret-key"));
    }

    #[test]
    fn saves_and_updates_chat_messages_in_one_timeline() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let message = database
            .insert_chat_message("user", "今天有点累", "pending")
            .expect("save message");
        database
            .update_chat_message_status(message.id, "sent")
            .expect("update message");
        database
            .insert_chat_message("assistant", "那就先歇一会儿，我陪你。", "sent")
            .expect("save reply");

        let messages = database.list_chat_messages().expect("list messages");
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].status, "sent");
        assert_eq!(messages[1].role, "assistant");
    }

    #[test]
    fn manages_memories_with_a_traceable_source_message() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let source = database
            .insert_chat_message("user", "我最近在准备项目方案", "sent")
            .expect("save source message");

        let memory = database
            .insert_memory("最近正在准备项目方案", source.id)
            .expect("save memory");
        assert_eq!(memory.source_message_id, source.id);
        assert_eq!(
            database.get_memory(memory.id).expect("read memory"),
            Some(memory.clone())
        );

        let updated = database
            .update_memory(memory.id, "最近在准备项目方案评审")
            .expect("update memory")
            .expect("memory exists");
        assert_eq!(updated.content, "最近在准备项目方案评审");
        assert_eq!(updated.source_message_id, source.id);
        assert_eq!(
            database.list_memories().expect("list memories"),
            vec![updated]
        );

        assert!(database.delete_memory(memory.id).expect("delete memory"));
        assert!(
            !database
                .delete_memory(memory.id)
                .expect("delete absent memory")
        );
        assert!(
            database
                .get_memory(memory.id)
                .expect("read deleted memory")
                .is_none()
        );
    }

    #[test]
    fn memory_source_foreign_key_prevents_orphaned_records() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");

        let error = database
            .insert_memory("没有来源的记忆", 42)
            .expect_err("orphan memory should be rejected");
        assert!(error.to_string().contains("failed to save memory"));
    }

    #[test]
    fn manages_confirmed_schedules_in_chronological_order() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let source = database
            .insert_chat_message("user", "明天九点半提醒我开评审会", "sent")
            .expect("save source message");
        let later = database
            .insert_schedule(
                "傍晚散步",
                "2026-09-18T18:30:00+08:00",
                "2026-09-18T18:20:00+08:00",
                None,
                "scheduled",
            )
            .expect("save later schedule");
        let schedule = database
            .insert_schedule(
                "项目方案评审会",
                "2026-09-18T09:30:00+08:00",
                "2026-09-18T09:00:00+08:00",
                Some(source.id),
                "scheduled",
            )
            .expect("save confirmed schedule");

        assert_eq!(
            database
                .list_schedules()
                .expect("list schedules")
                .iter()
                .map(|record| record.id)
                .collect::<Vec<_>>(),
            vec![schedule.id, later.id]
        );

        let updated = database
            .update_schedule(
                schedule.id,
                "项目方案评审",
                "2026-09-18T10:00:00+08:00",
                "2026-09-18T09:30:00+08:00",
                "scheduled",
            )
            .expect("update schedule")
            .expect("schedule exists");
        assert_eq!(updated.title, "项目方案评审");
        assert_eq!(updated.source_message_id, Some(source.id));
        assert!(database.delete_schedule(later.id).expect("delete schedule"));
        assert!(
            database
                .get_schedule(later.id)
                .expect("read deleted schedule")
                .is_none()
        );
    }

    #[test]
    fn deleting_a_source_message_detaches_but_keeps_its_schedule() {
        let directory = tempdir().expect("temporary directory");
        let database = Database::initialize(directory.path()).expect("database should initialize");
        let source = database
            .insert_chat_message("user", "周五给家人打电话", "sent")
            .expect("save source message");
        let schedule = database
            .insert_schedule(
                "给家人打电话",
                "2026-09-20T18:30:00+08:00",
                "2026-09-20T18:20:00+08:00",
                Some(source.id),
                "scheduled",
            )
            .expect("save schedule");

        database
            .connection()
            .expect("connection")
            .execute("DELETE FROM chat_messages WHERE id = ?1", [source.id])
            .expect("delete source message");

        assert_eq!(
            database
                .get_schedule(schedule.id)
                .expect("read schedule")
                .expect("schedule remains")
                .source_message_id,
            None
        );
    }
}
