use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tauri::{PhysicalPosition, PhysicalSize, State, WebviewWindow};

use crate::{
    error::{AppError, CommandError},
    infrastructure::{
        credentials,
        database::{
            ApiProfileRecord, AppSettings, ChatMessage, Database, PersonaProfile, WindowState,
        },
    },
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
    let models_url = format!("{}/models", profile.base_url.trim_end_matches('/'));

    let result =
        tauri::async_runtime::spawn_blocking(move || test_connection(&models_url, &api_key))
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

fn test_connection(url: &str, api_key: &str) -> Result<ApiTestResult, CommandError> {
    let client_builder = reqwest::blocking::Client::builder().timeout(Duration::from_secs(15));
    #[cfg(test)]
    let client_builder = client_builder.no_proxy();
    let client = client_builder
        .build()
        .map_err(|error| AppError::internal(format!("无法创建网络客户端：{error}")))?;
    let started = Instant::now();
    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .map_err(|error| AppError::Network(format!("连接 API 失败：{error}")))?;
    let status = response.status();
    if status.as_u16() == 401 || status.as_u16() == 403 {
        return Err(AppError::Authorization("API Key 无效或没有访问权限".to_owned()).into());
    }
    if !status.is_success() {
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
    complete_chat(&database, ChatMessage { status: "pending".to_owned(), ..message }).await
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
        let result = super::test_connection(&url, "test-secret").expect("connection succeeds");
        server.join().expect("mock API exits");
        assert!(result.success);
    }

    #[test]
    fn maps_unauthorized_api_responses() {
        let (url, server) = mock_api("401 Unauthorized");
        let error = super::test_connection(&url, "test-secret").expect_err("connection fails");
        server.join().expect("mock API exits");
        assert!(matches!(error.category, ErrorCategory::Authorization));
        assert!(!error.retryable);
    }
}
