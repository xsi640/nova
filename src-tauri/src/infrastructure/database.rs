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
                "SELECT theme, dark_mode, dnd_start, dnd_end, voice_autoplay, proactive_enabled
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
                    proactive_enabled = ?6
                 WHERE id = 1",
                params![
                    settings.theme,
                    settings.dark_mode,
                    settings.dnd_start,
                    settings.dnd_end,
                    settings.voice_autoplay,
                    settings.proactive_enabled,
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
            .map_err(|error| AppError::database(format!("failed to prepare chat query: {error}")))?;
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
            .map_err(|error| AppError::database(format!("failed to decode chat messages: {error}")))?;
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

    fn connection(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.connection
            .lock()
            .map_err(|_| AppError::database("database connection lock was poisoned"))
    }
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
        };
        database.save_settings(&settings).expect("save settings");
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
}
