use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use reqwest::{
    StatusCode,
    blocking::multipart::{Form, Part},
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, PhysicalPosition, PhysicalSize, State, WebviewWindow};
use tauri_plugin_notification::NotificationExt;

use crate::{
    error::{AppError, CommandError},
    infrastructure::{
        credentials,
        database::{
            ApiProfileRecord, AppSettings, ChatMessage, Database, MemoryRecord, PersonaProfile,
            ScheduleRecord, WindowState,
        },
    },
    schedule_intent::{LocalDate, parse_schedule_intent},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootstrapResponse {
    app_version: &'static str,
    onboarding_complete: bool,
    database_ready: bool,
    window_mode: String,
}

#[tauri::command]
pub fn bootstrap(database: State<'_, Database>) -> Result<BootstrapResponse, CommandError> {
    let persona_exists = database.get_persona()?.is_some();
    let chat_ready = database.get_api_profile("chat")?.is_some_and(|profile| {
        profile.last_tested_at.is_some()
            && credentials::secret_exists(&profile.secret_ref).unwrap_or(false)
    });
    let window_mode = database.get_window_state()?.mode;
    Ok(BootstrapResponse {
        app_version: env!("CARGO_PKG_VERSION"),
        onboarding_complete: persona_exists && chat_ready,
        database_ready: database.is_ready(),
        window_mode,
    })
}

#[tauri::command]
pub fn get_persona(database: State<'_, Database>) -> Result<Option<PersonaProfile>, CommandError> {
    Ok(database.get_persona()?)
}

#[tauri::command]
pub fn save_persona(
    database: State<'_, Database>,
    mut persona: PersonaProfile,
) -> Result<PersonaProfile, CommandError> {
    persona.name = required_text("姓名", persona.name, 32)?;
    persona.personality = required_text("性格", persona.personality, 240)?;
    persona.speech_style = required_text("说话方式", persona.speech_style, 240)?;
    database.save_persona(&persona)?;
    Ok(persona)
}

#[tauri::command]
pub fn get_settings(database: State<'_, Database>) -> Result<AppSettings, CommandError> {
    Ok(database.get_settings()?)
}

#[tauri::command]
pub fn save_settings(
    database: State<'_, Database>,
    settings: AppSettings,
) -> Result<AppSettings, CommandError> {
    let allowed_themes = ["rose", "lavender", "mint", "blue", "peach"];
    if !allowed_themes.contains(&settings.theme.as_str()) {
        return Err(AppError::Configuration("不支持的主题色".to_owned()).into());
    }
    validate_time_range(&settings.dnd_start, &settings.dnd_end)?;
    database.save_settings(&settings)?;
    Ok(settings)
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ApiCapability {
    Chat,
    Transcription,
    Speech,
}

impl ApiCapability {
    fn as_str(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Transcription => "transcription",
            Self::Speech => "speech",
        }
    }

    fn secret_reference(self) -> String {
        format!("api-{}", self.as_str())
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfileInput {
    capability: ApiCapability,
    base_url: String,
    path: String,
    model: String,
    api_key: Option<String>,
    enabled: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiProfileStatus {
    capability: ApiCapability,
    base_url: String,
    path: String,
    model: String,
    has_api_key: bool,
    enabled: bool,
    connection_tested: bool,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiTestResult {
    success: bool,
    latency_ms: u128,
}

#[tauri::command]
pub fn get_api_profile_status(
    database: State<'_, Database>,
    capability: ApiCapability,
) -> Result<Option<ApiProfileStatus>, CommandError> {
    let Some(profile) = database.get_api_profile(capability.as_str())? else {
        return Ok(None);
    };
    Ok(Some(profile_status(capability, profile)?))
}

#[tauri::command]
pub fn save_api_profile(
    database: State<'_, Database>,
    mut profile: ApiProfileInput,
) -> Result<ApiProfileStatus, CommandError> {
    profile.base_url = validate_base_url(profile.base_url)?;
    profile.path = required_text("接口路径", profile.path, 160)?;
    if !profile.path.starts_with('/') {
        return Err(AppError::Configuration("接口路径必须以 / 开头".to_owned()).into());
    }
    profile.model = required_text("模型", profile.model, 120)?;

    let secret_ref = profile.capability.secret_reference();
    let existing_secret = credentials::secret_exists(&secret_ref)?;
    let new_secret = profile
        .api_key
        .take()
        .map(|secret| secret.trim().to_owned())
        .filter(|secret| !secret.is_empty());
    if new_secret.is_none() && !existing_secret {
        return Err(AppError::Configuration("请填写 API Key".to_owned()).into());
    }

    let record = ApiProfileRecord {
        capability: profile.capability.as_str().to_owned(),
        base_url: profile.base_url,
        path: profile.path,
        model: profile.model,
        secret_ref: secret_ref.clone(),
        enabled: profile.enabled,
        last_tested_at: None,
    };
    database.save_api_profile(&record)?;
    if let Some(secret) = new_secret {
        credentials::set_secret(&secret_ref, &secret)?;
    }
    profile_status(profile.capability, record)
}

#[tauri::command]
pub async fn test_api_profile(
    database: State<'_, Database>,
    capability: ApiCapability,
) -> Result<ApiTestResult, CommandError> {
    let profile = database
        .get_api_profile(capability.as_str())?
        .ok_or_else(|| AppError::Configuration("请先保存 API 配置".to_owned()))?;
    let api_key = credentials::get_secret(&profile.secret_ref)?;
    let endpoint = format!("{}{}", profile.base_url.trim_end_matches('/'), profile.path);

    let result = tauri::async_runtime::spawn_blocking(move || {
        test_capability_connection(&endpoint, &api_key)
    })
    .await
    .map_err(|error| AppError::internal(format!("连接测试任务失败：{error}")))??;
    database.mark_api_profile_tested(capability.as_str())?;
    Ok(result)
}

fn profile_status(
    capability: ApiCapability,
    profile: ApiProfileRecord,
) -> Result<ApiProfileStatus, CommandError> {
    let has_api_key = credentials::secret_exists(&profile.secret_ref)?;
    Ok(ApiProfileStatus {
        capability,
        base_url: profile.base_url,
        path: profile.path,
        model: profile.model,
        has_api_key,
        enabled: profile.enabled,
        connection_tested: profile.last_tested_at.is_some(),
    })
}

fn validate_base_url(value: String) -> Result<String, CommandError> {
    let value = value.trim().trim_end_matches('/').to_owned();
    let parsed = reqwest::Url::parse(&value)
        .map_err(|_| AppError::Configuration("接口地址不是有效 URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(AppError::Configuration("接口地址必须使用 http 或 https".to_owned()).into());
    }
    Ok(value)
}

/// Test the configured capability endpoint without invoking a model or uploading user audio.
///
/// OpenAI-compatible providers commonly answer `OPTIONS` with either a successful CORS
/// response or `405 Method Not Allowed`. Both prove that the configured endpoint is reachable;
/// an authorization response is still surfaced as an invalid key. This deliberately avoids a
/// billable transcription or speech-generation request during settings validation.
fn test_capability_connection(url: &str, api_key: &str) -> Result<ApiTestResult, CommandError> {
    let client = http_client(Duration::from_secs(15))?;
    let started = Instant::now();
    let response = client
        .request(reqwest::Method::OPTIONS, url)
        .bearer_auth(api_key)
        .send()
        .map_err(|error| AppError::Network(format!("连接 API 失败：{error}")))?;
    let status = response.status();
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(AppError::Authorization("API Key 无效或没有访问权限".to_owned()).into());
    }
    if !status.is_success() && status != StatusCode::METHOD_NOT_ALLOWED {
        return Err(AppError::Service(format!("API 返回状态码 {}", status.as_u16())).into());
    }
    Ok(ApiTestResult {
        success: true,
        latency_ms: started.elapsed().as_millis(),
    })
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatExchange {
    user_message: ChatMessage,
    assistant_message: ChatMessage,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<RemoteChatMessage>,
}

#[derive(Debug, Serialize)]
struct RemoteChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionChoice {
    message: ChatCompletionMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionMessage {
    content: String,
}

#[tauri::command]
pub fn list_messages(database: State<'_, Database>) -> Result<Vec<ChatMessage>, CommandError> {
    Ok(database.list_chat_messages()?)
}

#[tauri::command]
pub fn list_memories(database: State<'_, Database>) -> Result<Vec<MemoryRecord>, CommandError> {
    Ok(database.list_memories()?)
}

#[tauri::command]
pub fn update_memory(
    database: State<'_, Database>,
    id: i64,
    content: String,
) -> Result<MemoryRecord, CommandError> {
    let content = required_text("记忆内容", content, 600)?;
    database
        .update_memory(id, &content)?
        .ok_or_else(|| AppError::Configuration("找不到要修改的记忆".to_owned()).into())
}

#[tauri::command]
pub fn delete_memory(database: State<'_, Database>, id: i64) -> Result<bool, CommandError> {
    Ok(database.delete_memory(id)?)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleInput {
    title: String,
    scheduled_at: String,
    remind_at: String,
    source_message_id: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleUpdateInput {
    id: i64,
    title: String,
    scheduled_at: String,
    remind_at: String,
    status: String,
}

#[tauri::command]
pub fn list_schedules(database: State<'_, Database>) -> Result<Vec<ScheduleRecord>, CommandError> {
    Ok(database.list_schedules()?)
}

/// Persists a schedule only after the user has explicitly confirmed the details in the UI.
#[tauri::command]
pub fn confirm_schedule(
    database: State<'_, Database>,
    input: ScheduleInput,
) -> Result<ScheduleRecord, CommandError> {
    let (title, scheduled_at, remind_at) =
        validate_schedule_fields(input.title, input.scheduled_at, input.remind_at)?;
    Ok(database.insert_schedule(
        &title,
        &scheduled_at,
        &remind_at,
        input.source_message_id,
        "scheduled",
    )?)
}

#[tauri::command]
pub fn update_schedule(
    database: State<'_, Database>,
    input: ScheduleUpdateInput,
) -> Result<ScheduleRecord, CommandError> {
    let (title, scheduled_at, remind_at) =
        validate_schedule_fields(input.title, input.scheduled_at, input.remind_at)?;
    if !matches!(
        input.status.as_str(),
        "scheduled" | "completed" | "cancelled"
    ) {
        return Err(AppError::Configuration("日程状态无效".to_owned()).into());
    }
    database
        .update_schedule(input.id, &title, &scheduled_at, &remind_at, &input.status)?
        .ok_or_else(|| AppError::Configuration("找不到要修改的日程".to_owned()).into())
}

#[tauri::command]
pub fn delete_schedule(database: State<'_, Database>, id: i64) -> Result<bool, CommandError> {
    Ok(database.delete_schedule(id)?)
}

/// Returns the privacy-preserving JSON export for the UI to save to a user-selected file.
/// The export module uses an explicit allow-list and never reads credentials or API settings.
#[tauri::command]
pub fn export_local_data(database: State<'_, Database>) -> Result<String, CommandError> {
    crate::export::local_data_export_json(&database).map_err(Into::into)
}

/// Hands a user-visible notification to the operating system. Callers supply only already
/// rendered text; no API credentials, message history, or input activity leave the app.
#[tauri::command]
pub fn show_notification(app: AppHandle, title: String, body: String) -> Result<(), CommandError> {
    let title = required_text("通知标题", title, 120)?;
    let body = required_text("通知内容", body, 500)?;
    app.notification()
        .builder()
        .title(title)
        .body(body)
        .show()
        .map_err(|error| AppError::PlatformPermission(format!("无法显示系统通知：{error}")))?;
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCandidateInput {
    content: String,
    source_message_id: i64,
    year: i32,
    month: u8,
    day: u8,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleCandidate {
    title: String,
    scheduled_at: String,
    remind_at: String,
    source_message_id: i64,
}

/// Parses only unambiguous, explicitly requested local schedules. It returns a candidate; the
/// UI must call `confirm_schedule` only after the user accepts the confirmation card.
#[tauri::command]
pub fn get_schedule_candidate(
    database: State<'_, Database>,
    input: ScheduleCandidateInput,
) -> Result<Option<ScheduleCandidate>, CommandError> {
    let source = database
        .get_chat_message(input.source_message_id)?
        .filter(|message| message.role == "user" && message.status == "sent")
        .ok_or_else(|| AppError::Configuration("找不到对应的已发送用户消息".to_owned()))?;
    if source.content != input.content {
        return Err(AppError::Configuration("日程候选的消息内容不匹配".to_owned()).into());
    }
    let today = LocalDate::new(input.year, input.month, input.day)
        .ok_or_else(|| AppError::Configuration("本地日期无效".to_owned()))?;
    Ok(
        parse_schedule_intent(&source.content, today).map(|intent| ScheduleCandidate {
            title: intent.title,
            scheduled_at: intent.scheduled_at,
            remind_at: intent.remind_at,
            source_message_id: input.source_message_id,
        }),
    )
}

#[tauri::command]
pub async fn send_message(
    database: State<'_, Database>,
    content: String,
) -> Result<ChatExchange, CommandError> {
    let content = required_text("消息", content, 8_000)?;
    let user_message = database.insert_chat_message("user", &content, "pending")?;
    complete_chat(&database, user_message).await
}

#[tauri::command]
pub async fn retry_message(
    database: State<'_, Database>,
    message_id: i64,
) -> Result<ChatExchange, CommandError> {
    let message = database
        .get_chat_message(message_id)?
        .ok_or_else(|| AppError::Configuration("找不到要重试的消息".to_owned()))?;
    if message.role != "user" || message.status != "failed" {
        return Err(AppError::Configuration("这条消息当前不能重试".to_owned()).into());
    }
    database.update_chat_message_status(message.id, "pending")?;
    complete_chat(
        &database,
        ChatMessage {
            status: "pending".to_owned(),
            ..message
        },
    )
    .await
}

async fn complete_chat(
    database: &Database,
    user_message: ChatMessage,
) -> Result<ChatExchange, CommandError> {
    let result = async {
        let profile = database
            .get_api_profile("chat")?
            .filter(|profile| profile.enabled)
            .ok_or_else(|| AppError::Configuration("请先配置并启用对话 API".to_owned()))?;
        let api_key = credentials::get_secret(&profile.secret_ref)?;
        let persona = database
            .get_persona()?
            .ok_or_else(|| AppError::Configuration("请先完成角色设定".to_owned()))?;
        let mut messages = vec![RemoteChatMessage {
            role: "system".to_owned(),
            content: format!(
                "你是用户的长期 AI 陪伴者，名字是{}。你的性格是：{}。你的说话方式是：{}。请保持真诚、自然和有边界感，不要声称自己是真人。",
                persona.name, persona.personality, persona.speech_style
            ),
        }];
        messages.extend(
            database
                .list_chat_messages()?
                .into_iter()
                .filter(|message| {
                    (message.status == "sent" || message.id == user_message.id)
                        && matches!(message.role.as_str(), "user" | "assistant")
                })
                .rev()
                .take(40)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .map(|message| RemoteChatMessage {
                    role: message.role,
                    content: message.content,
                }),
        );
        let endpoint = format!(
            "{}{}",
            profile.base_url.trim_end_matches('/'),
            profile.path
        );
        let request = ChatCompletionRequest {
            model: profile.model,
            messages,
        };
        let reply = tauri::async_runtime::spawn_blocking(move || {
            request_chat_completion(&endpoint, &api_key, &request)
        })
        .await
        .map_err(|error| AppError::internal(format!("对话任务失败：{error}")))??;

        database.update_chat_message_status(user_message.id, "sent")?;
        let user_message = ChatMessage {
            status: "sent".to_owned(),
            ..user_message.clone()
        };
        let assistant_message = database.insert_chat_message("assistant", &reply, "sent")?;
        persist_memory_candidate(database, &user_message);
        Ok(ChatExchange {
            user_message,
            assistant_message,
        })
    }
    .await;

    if result.is_err() {
        database.update_chat_message_status(user_message.id, "failed")?;
    }
    result
}

/// Save only explicit, durable facts. This conservative local extraction is deliberately
/// best-effort: it never delays or invalidates an otherwise successful conversation, and users
/// can edit or delete every resulting record from the memory page.
fn persist_memory_candidate(database: &Database, source: &ChatMessage) {
    let Some(content) = extract_memory_candidate(&source.content) else {
        return;
    };
    let duplicate = database
        .list_memories()
        .map(|memories| memories.iter().any(|memory| memory.content == content))
        .unwrap_or(true);
    if !duplicate {
        let _ = database.insert_memory(&content, source.id);
    }
}

fn extract_memory_candidate(message: &str) -> Option<String> {
    let sentence = message
        .trim()
        .split(['。', '！', '？', '\n'])
        .next()
        .unwrap_or_default()
        .trim_matches(|character: char| {
            character.is_whitespace() || matches!(character, '，' | ',' | '。')
        })
        .trim();
    let explicit = sentence
        .strip_prefix("记住")
        .map(str::trim)
        .map(|value| {
            value.trim_start_matches(|character: char| matches!(character, '：' | ':' | '，' | ','))
        })
        .filter(|value| !value.is_empty())
        .unwrap_or(sentence);
    let is_durable = sentence.starts_with("记住")
        || explicit.starts_with("我喜欢")
        || explicit.starts_with("我不喜欢")
        || explicit.starts_with("我叫")
        || explicit.starts_with("我住在")
        || explicit.starts_with("我的生日");
    let length = explicit.chars().count();
    (is_durable && (2..=240).contains(&length)).then(|| explicit.to_owned())
}

fn request_chat_completion(
    url: &str,
    api_key: &str,
    request: &ChatCompletionRequest,
) -> Result<String, CommandError> {
    let client = http_client(Duration::from_secs(90))?;
    let response = client
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .map_err(|error| AppError::Network(format!("发送消息失败：{error}")))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::Authorization("API Key 无效或没有访问权限".to_owned()).into());
    }
    if !status.is_success() {
        return Err(AppError::Service(format!("对话 API 返回状态码 {}", status.as_u16())).into());
    }
    let response = response
        .json::<ChatCompletionResponse>()
        .map_err(|error| AppError::Service(format!("无法解析对话 API 响应：{error}")))?;
    let content = response
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content.trim().to_owned())
        .filter(|content| !content.is_empty())
        .ok_or_else(|| AppError::Service("对话 API 没有返回文字内容".to_owned()))?;
    Ok(content)
}

fn http_client(timeout: Duration) -> Result<reqwest::blocking::Client, CommandError> {
    let client_builder = reqwest::blocking::Client::builder().timeout(timeout);
    #[cfg(test)]
    let client_builder = client_builder.no_proxy();
    client_builder
        .build()
        .map_err(|error| AppError::internal(format!("无法创建网络客户端：{error}")).into())
}

const MAX_AUDIO_UPLOAD_BYTES: usize = 25 * 1024 * 1024;
const MAX_SYNTHESIZED_AUDIO_BYTES: usize = 25 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionResult {
    text: String,
}

#[derive(Debug, Deserialize)]
struct TranscriptionResponse {
    text: String,
}

#[derive(Debug, Serialize)]
struct SpeechSynthesisRequest {
    model: String,
    input: String,
    voice: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SpeechSynthesisResult {
    /// Base64 keeps the binary payload inside Tauri's typed command boundary. It is intended for
    /// an in-memory browser `Audio` object and is never persisted to the chat database.
    audio_base64: String,
    content_type: String,
}

#[tauri::command]
pub async fn transcribe_audio(
    database: State<'_, Database>,
    audio: Vec<u8>,
    file_name: Option<String>,
    mime_type: Option<String>,
) -> Result<TranscriptionResult, CommandError> {
    if audio.is_empty() {
        return Err(AppError::Audio("没有可供识别的录音".to_owned()).into());
    }
    if audio.len() > MAX_AUDIO_UPLOAD_BYTES {
        return Err(AppError::Audio("录音文件不能超过 25 MB".to_owned()).into());
    }

    let profile = enabled_audio_profile(&database, "transcription", "语音识别")?;
    let api_key = credentials::get_secret(&profile.secret_ref)?;
    let endpoint = format!("{}{}", profile.base_url.trim_end_matches('/'), profile.path);
    let file_name = validate_audio_file_name(file_name)?;
    let mime_type = validate_audio_mime_type(mime_type)?;
    let model = profile.model;
    let result = tauri::async_runtime::spawn_blocking(move || {
        request_transcription(&endpoint, &api_key, &model, audio, &file_name, &mime_type)
    })
    .await
    .map_err(|error| AppError::internal(format!("语音识别任务失败：{error}")))??;
    Ok(TranscriptionResult { text: result })
}

#[tauri::command]
pub async fn synthesize_speech(
    database: State<'_, Database>,
    text: String,
    voice: Option<String>,
) -> Result<SpeechSynthesisResult, CommandError> {
    let input = required_text("要朗读的文本", text, 8_000)?;
    let voice = required_text("音色", voice.unwrap_or_else(|| "alloy".to_owned()), 64)?;
    let profile = enabled_audio_profile(&database, "speech", "语音合成")?;
    let api_key = credentials::get_secret(&profile.secret_ref)?;
    let endpoint = format!("{}{}", profile.base_url.trim_end_matches('/'), profile.path);
    let request = SpeechSynthesisRequest {
        model: profile.model,
        input,
        voice,
    };
    let result = tauri::async_runtime::spawn_blocking(move || {
        request_speech_synthesis(&endpoint, &api_key, &request)
    })
    .await
    .map_err(|error| AppError::internal(format!("语音合成任务失败：{error}")))??;
    Ok(result)
}

fn enabled_audio_profile(
    database: &Database,
    capability: &str,
    label: &str,
) -> Result<ApiProfileRecord, CommandError> {
    database
        .get_api_profile(capability)?
        .filter(|profile| profile.enabled)
        .ok_or_else(|| AppError::Configuration(format!("请先配置并启用{label} API")).into())
}

fn validate_audio_file_name(file_name: Option<String>) -> Result<String, CommandError> {
    let file_name = file_name
        .unwrap_or_else(|| "recording.webm".to_owned())
        .trim()
        .to_owned();
    if file_name.is_empty()
        || file_name.chars().count() > 120
        || file_name.contains(['/', '\\', '\0'])
    {
        return Err(AppError::Audio("录音文件名无效".to_owned()).into());
    }
    Ok(file_name)
}

fn validate_audio_mime_type(mime_type: Option<String>) -> Result<String, CommandError> {
    let mime_type = mime_type
        .unwrap_or_else(|| "audio/webm".to_owned())
        .trim()
        .to_ascii_lowercase();
    if !matches!(
        mime_type.as_str(),
        "audio/webm"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/mpeg"
            | "audio/mp4"
            | "audio/ogg"
            | "audio/flac"
            | "audio/x-m4a"
    ) {
        return Err(AppError::Audio("不支持该录音格式".to_owned()).into());
    }
    Ok(mime_type)
}

fn request_transcription(
    url: &str,
    api_key: &str,
    model: &str,
    audio: Vec<u8>,
    file_name: &str,
    mime_type: &str,
) -> Result<String, CommandError> {
    let file = Part::bytes(audio)
        .file_name(file_name.to_owned())
        .mime_str(mime_type)
        .map_err(|error| AppError::Audio(format!("无法处理录音格式：{error}")))?;
    let form = Form::new()
        .text("model", model.to_owned())
        .part("file", file);
    let response = http_client(Duration::from_secs(90))?
        .post(url)
        .bearer_auth(api_key)
        .multipart(form)
        .send()
        .map_err(|error| AppError::Network(format!("发送录音失败：{error}")))?;
    validate_audio_api_response(response.status(), "语音识别")?;
    let response = response
        .json::<TranscriptionResponse>()
        .map_err(|error| AppError::Service(format!("无法解析语音识别 API 响应：{error}")))?;
    required_text("识别结果", response.text, 8_000)
}

fn request_speech_synthesis(
    url: &str,
    api_key: &str,
    request: &SpeechSynthesisRequest,
) -> Result<SpeechSynthesisResult, CommandError> {
    let response = http_client(Duration::from_secs(90))?
        .post(url)
        .bearer_auth(api_key)
        .json(request)
        .send()
        .map_err(|error| AppError::Network(format!("请求语音合成失败：{error}")))?;
    validate_audio_api_response(response.status(), "语音合成")?;
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("audio/mpeg")
        .split(';')
        .next()
        .unwrap_or("audio/mpeg")
        .trim()
        .to_ascii_lowercase();
    if !content_type.starts_with("audio/") && content_type != "application/octet-stream" {
        return Err(AppError::Audio("语音合成 API 没有返回音频数据".to_owned()).into());
    }
    let bytes = response
        .bytes()
        .map_err(|error| AppError::Network(format!("读取合成音频失败：{error}")))?;
    if bytes.is_empty() {
        return Err(AppError::Audio("语音合成 API 返回了空音频".to_owned()).into());
    }
    if bytes.len() > MAX_SYNTHESIZED_AUDIO_BYTES {
        return Err(AppError::Audio("合成音频超过 25 MB，无法播放".to_owned()).into());
    }
    Ok(SpeechSynthesisResult {
        audio_base64: BASE64.encode(bytes),
        content_type,
    })
}

fn validate_audio_api_response(status: StatusCode, capability: &str) -> Result<(), CommandError> {
    if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
        return Err(AppError::Authorization("API Key 无效或没有访问权限".to_owned()).into());
    }
    if !status.is_success() {
        return Err(
            AppError::Service(format!("{capability} API 返回状态码 {}", status.as_u16())).into(),
        );
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WindowMode {
    Compact,
    Management,
}

impl WindowMode {
    fn dimensions(self) -> (&'static str, u32, u32, u32, u32) {
        match self {
            Self::Compact => ("compact", 440, 760, 400, 560),
            Self::Management => ("management", 1080, 760, 760, 560),
        }
    }
}

#[tauri::command]
pub fn set_window_mode(
    window: WebviewWindow,
    database: State<'_, Database>,
    mode: WindowMode,
) -> Result<(), CommandError> {
    let (name, width, height, minimum_width, minimum_height) = mode.dimensions();
    window
        .set_min_size(Some(PhysicalSize::new(minimum_width, minimum_height)))
        .map_err(|error| AppError::internal(format!("无法设置窗口最小尺寸：{error}")))?;
    window
        .set_size(PhysicalSize::new(width, height))
        .map_err(|error| AppError::internal(format!("无法切换窗口模式：{error}")))?;
    database.save_window_mode(name, width, height)?;
    Ok(())
}

pub fn restore_window_state(window: &WebviewWindow, state: &WindowState) -> Result<(), AppError> {
    let (minimum_width, minimum_height) = if state.mode == "compact" {
        (400, 560)
    } else {
        (760, 560)
    };
    window
        .set_min_size(Some(PhysicalSize::new(minimum_width, minimum_height)))
        .map_err(|error| AppError::internal(format!("failed to restore minimum size: {error}")))?;
    window
        .set_size(PhysicalSize::new(state.width, state.height))
        .map_err(|error| AppError::internal(format!("failed to restore window size: {error}")))?;
    if let (Some(x), Some(y)) = (state.x, state.y) {
        window
            .set_position(PhysicalPosition::new(x, y))
            .map_err(|error| {
                AppError::internal(format!("failed to restore window position: {error}"))
            })?;
    }
    Ok(())
}

fn validate_schedule_fields(
    title: String,
    scheduled_at: String,
    remind_at: String,
) -> Result<(String, String, String), CommandError> {
    let title = required_text("日程标题", title, 160)?;
    let scheduled_at = validate_local_datetime("日程时间", scheduled_at)?;
    let remind_at = validate_local_datetime("提醒时间", remind_at)?;
    if remind_at > scheduled_at {
        return Err(AppError::Configuration("提醒时间不能晚于日程时间".to_owned()).into());
    }
    Ok((title, scheduled_at, remind_at))
}

/// The UI sends the value from a native `datetime-local` control. Keep it as local wall-clock
/// time: application schedules deliberately do not read or modify the system calendar.
fn validate_local_datetime(label: &str, value: String) -> Result<String, CommandError> {
    let value = value.trim().to_owned();
    let valid = value.len() == 16
        && value.as_bytes().get(4) == Some(&b'-')
        && value.as_bytes().get(7) == Some(&b'-')
        && value.as_bytes().get(10) == Some(&b'T')
        && value.as_bytes().get(13) == Some(&b':')
        && value.bytes().enumerate().all(|(index, character)| {
            matches!(index, 4 | 7 | 10 | 13) || character.is_ascii_digit()
        });
    if !valid {
        return Err(AppError::Configuration(format!("{label}格式应为 YYYY-MM-DDTHH:MM")).into());
    }
    let parse = |range: std::ops::Range<usize>| value[range].parse::<u8>();
    let month = parse(5..7).ok();
    let day = parse(8..10).ok();
    let hour = parse(11..13).ok();
    let minute = parse(14..16).ok();
    if !matches!(month, Some(1..=12))
        || !matches!(day, Some(1..=31))
        || !matches!(hour, Some(0..=23))
        || !matches!(minute, Some(0..=59))
    {
        return Err(AppError::Configuration(format!("{label}不是有效时间")).into());
    }
    Ok(value)
}

fn required_text(label: &str, value: String, max_chars: usize) -> Result<String, CommandError> {
    let value = value.trim().to_owned();
    if value.is_empty() {
        return Err(AppError::Configuration(format!("{label}不能为空")).into());
    }
    if value.chars().count() > max_chars {
        return Err(AppError::Configuration(format!("{label}不能超过 {max_chars} 个字符")).into());
    }
    Ok(value)
}

fn validate_time_range(start: &Option<String>, end: &Option<String>) -> Result<(), CommandError> {
    if start.is_some() != end.is_some() {
        return Err(AppError::Configuration("免打扰开始和结束时间需同时设置".to_owned()).into());
    }
    for value in [start, end].into_iter().flatten() {
        let valid = value.split_once(':').is_some_and(|(hour, minute)| {
            hour.len() == 2
                && minute.len() == 2
                && hour.parse::<u8>().is_ok_and(|hour| hour < 24)
                && minute.parse::<u8>().is_ok_and(|minute| minute < 60)
        });
        if !valid {
            return Err(AppError::Configuration("免打扰时间格式应为 HH:MM".to_owned()).into());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    use crate::error::ErrorCategory;

    use super::BootstrapResponse;

    fn mock_api(status: &str) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock API");
        let address = listener.local_addr().expect("mock API address");
        let status = status.to_owned();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 2048];
            let length = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..length]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer test-secret"));
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}"
            )
            .expect("write response");
        });
        (format!("http://{address}/models"), handle)
    }

    fn mock_audio_api(
        status: &str,
        content_type: &str,
        body: &[u8],
    ) -> (String, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock audio API");
        let address = listener.local_addr().expect("mock audio API address");
        let status = status.to_owned();
        let content_type = content_type.to_owned();
        let body = body.to_vec();
        let handle = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request = [0_u8; 4096];
            let length = stream.read(&mut request).expect("read request");
            let request = String::from_utf8_lossy(&request[..length]).to_ascii_lowercase();
            assert!(request.contains("authorization: bearer test-secret"));
            write!(
                stream,
                "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            )
            .expect("write response headers");
            stream.write_all(&body).expect("write response body");
        });
        (format!("http://{address}/audio"), handle)
    }

    #[test]
    fn bootstrap_response_serializes_camel_case_fields() {
        let response = BootstrapResponse {
            app_version: "0.1.0",
            onboarding_complete: false,
            database_ready: true,
            window_mode: "management".to_owned(),
        };

        assert!(!response.onboarding_complete);
        assert!(response.database_ready);
        assert_eq!(response.window_mode, "management");
    }

    #[test]
    fn validates_required_text() {
        let error = super::required_text("姓名", "  ".to_owned(), 32).expect_err("empty name");
        assert_eq!(error.code, "CONFIGURATION");
        assert_eq!(
            super::required_text("姓名", "  小诺  ".to_owned(), 32).expect("valid name"),
            "小诺"
        );
    }

    #[test]
    fn validates_quiet_hours_as_a_pair() {
        assert!(super::validate_time_range(&Some("23:00".to_owned()), &None).is_err());
        assert!(
            super::validate_time_range(&Some("23:00".to_owned()), &Some("08:00".to_owned()))
                .is_ok()
        );
        assert!(
            super::validate_time_range(&Some("25:00".to_owned()), &Some("08:00".to_owned()))
                .is_err()
        );
    }

    #[test]
    fn tests_an_openai_compatible_connection_without_exposing_the_secret() {
        let (url, server) = mock_api("200 OK");
        let result =
            super::test_capability_connection(&url, "test-secret").expect("connection succeeds");
        server.join().expect("mock API exits");
        assert!(result.success);
    }

    #[test]
    fn maps_unauthorized_api_responses() {
        let (url, server) = mock_api("401 Unauthorized");
        let error =
            super::test_capability_connection(&url, "test-secret").expect_err("connection fails");
        server.join().expect("mock API exits");
        assert!(matches!(error.category, ErrorCategory::Authorization));
        assert!(!error.retryable);
    }

    #[test]
    fn capability_connection_accepts_a_non_billable_method_not_allowed_response() {
        let (url, server) = mock_api("405 Method Not Allowed");
        let result = super::test_capability_connection(&url, "test-secret")
            .expect("endpoint reachability succeeds");
        server.join().expect("mock API exits");
        assert!(result.success);
    }

    #[test]
    fn transcription_uploads_audio_and_returns_trimmed_text() {
        let (url, server) = mock_audio_api(
            "200 OK",
            "application/json",
            r#"{"text":"  你好呀  "}"#.as_bytes(),
        );
        let text = super::request_transcription(
            &url,
            "test-secret",
            "transcription-model",
            vec![1, 2, 3],
            "recording.webm",
            "audio/webm",
        )
        .expect("transcription succeeds");
        server.join().expect("mock API exits");
        assert_eq!(text, "你好呀");
    }

    #[test]
    fn speech_synthesis_returns_base64_audio_with_a_safe_content_type() {
        let (url, server) = mock_audio_api("200 OK", "audio/mpeg; charset=binary", &[1, 2, 3]);
        let result = super::request_speech_synthesis(
            &url,
            "test-secret",
            &super::SpeechSynthesisRequest {
                model: "speech-model".to_owned(),
                input: "你好".to_owned(),
                voice: "alloy".to_owned(),
            },
        )
        .expect("speech synthesis succeeds");
        server.join().expect("mock API exits");
        assert_eq!(result.content_type, "audio/mpeg");
        assert_eq!(result.audio_base64, "AQID");
    }

    #[test]
    fn rejects_unsafe_audio_metadata_before_uploading_it() {
        assert!(super::validate_audio_file_name(Some("../recording.webm".to_owned())).is_err());
        assert!(super::validate_audio_mime_type(Some("text/html".to_owned())).is_err());
        assert_eq!(
            super::validate_audio_mime_type(None).expect("default MIME type"),
            "audio/webm"
        );
    }

    #[test]
    fn validates_schedule_time_fields_and_reminder_order() {
        assert!(
            super::validate_schedule_fields(
                "项目评审".to_owned(),
                "2026-09-18T10:00".to_owned(),
                "2026-09-18T09:30".to_owned(),
            )
            .is_ok()
        );
        assert!(
            super::validate_schedule_fields(
                "项目评审".to_owned(),
                "2026-09-18T10:00".to_owned(),
                "2026-09-18T10:01".to_owned(),
            )
            .is_err()
        );
        assert!(super::validate_local_datetime("日程时间", "2026-19-18T10:00".to_owned()).is_err());
    }

    #[test]
    fn extracts_only_explicit_durable_memory_candidates() {
        assert_eq!(
            super::extract_memory_candidate("记住：我喜欢周末去爬山。之后再聊"),
            Some("我喜欢周末去爬山".to_owned())
        );
        assert_eq!(
            super::extract_memory_candidate("我叫小苏，今天有点累"),
            Some("我叫小苏，今天有点累".to_owned())
        );
        assert_eq!(super::extract_memory_candidate("今天下雨了"), None);
    }
}
