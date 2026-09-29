//! Online Mandarin speech synthesis through Volcengine's Doubao (Seed-TTS 2.0) service.
//!
//! Volcengine's current voice service is the v3 bidirectional WebSocket protocol
//! (`wss://openspeech.bytedance.com/api/v3/tts/bidirection`). Nova connects, opens one session,
//! sends the whole utterance as a single `TaskRequest`, and concatenates the returned MP3 frames.
//! The signed console credentials (`App ID` + `Access Token`) never leave the machine: the
//! access token lives in the OS credential store, never in the database.
//!
//! Protocol reference: <https://www.volcengine.com/docs/6561/1329505>

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use native_tls::{TlsConnector, TlsStream};
use sha2::{Digest, Sha256};

use crate::{error::AppError, speech_text::prepare_for_speech};

/// Credential-store reference for the Volcengine access token.
pub const SECRET_REFERENCE: &str = "tts-volcengine";

const WS_HOST: &str = "openspeech.bytedance.com";
const WS_PATH: &str = "/api/v3/tts/bidirection";

pub const DEFAULT_RESOURCE_ID: &str = "seed-tts-2.0";
/// Seed-TTS 2.0 model versions. `seed-tts-2.0-expressive` is the more emotive variant.
pub const DEFAULT_MODEL: &str = "seed-tts-2.0-standard";
pub const DEFAULT_VOICE: &str = "zh_female_xiaohe_uranus_bigtts";
pub const DEFAULT_SAMPLE_RATE: u32 = 24_000;
pub const AUDIO_FORMAT: &str = "mp3";
pub const AUDIO_CONTENT_TYPE: &str = "audio/mpeg";
pub const RATE_RANGE: std::ops::RangeInclusive<i32> = -50..=100;
pub const LOUDNESS_RANGE: std::ops::RangeInclusive<i32> = -50..=100;

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const READ_TIMEOUT: Duration = Duration::from_secs(45);
const WRITE_TIMEOUT: Duration = Duration::from_secs(15);
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

// Message types (high nibble of byte 1).
const MSG_FULL_CLIENT_REQUEST: u8 = 0b0001;
const MSG_FULL_SERVER_RESPONSE: u8 = 0b1001;
const MSG_AUDIO_ONLY_RESPONSE: u8 = 0b1011;
const MSG_ERROR_INFORMATION: u8 = 0b1111;
// Message type specific flags (low nibble of byte 1).
const FLAG_POSITIVE_SEQ: u8 = 0b0001;
const FLAG_NEGATIVE_SEQ: u8 = 0b0011;
const FLAG_WITH_EVENT: u8 = 0b0100;
const SERIALIZATION_JSON: u8 = 0b0001;

// Events.
const EVENT_START_CONNECTION: i32 = 1;
const EVENT_FINISH_CONNECTION: i32 = 2;
const EVENT_CONNECTION_STARTED: i32 = 50;
const EVENT_CONNECTION_FAILED: i32 = 51;
const EVENT_CONNECTION_FINISHED: i32 = 52;
const EVENT_START_SESSION: i32 = 100;
const EVENT_FINISH_SESSION: i32 = 102;
const EVENT_SESSION_STARTED: i32 = 150;
const EVENT_SESSION_FINISHED: i32 = 152;
const EVENT_SESSION_FAILED: i32 = 153;
const EVENT_TASK_REQUEST: i32 = 200;
const EVENT_TTS_RESPONSE: i32 = 352;
const EVENT_TTS_ENDED: i32 = 359;

const CONNECTION_EVENTS: [i32; 5] = [
    EVENT_START_CONNECTION,
    EVENT_FINISH_CONNECTION,
    EVENT_CONNECTION_STARTED,
    EVENT_CONNECTION_FAILED,
    EVENT_CONNECTION_FINISHED,
];

#[derive(Debug, Clone, PartialEq)]
pub struct VolcOptions {
    pub resource_id: String,
    pub model: String,
    pub voice: String,
    pub speech_rate: i32,
    pub loudness_rate: i32,
}

impl Default for VolcOptions {
    fn default() -> Self {
        Self {
            resource_id: DEFAULT_RESOURCE_ID.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            voice: DEFAULT_VOICE.to_owned(),
            speech_rate: 0,
            loudness_rate: 0,
        }
    }
}

#[derive(Debug)]
pub struct Synthesis {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub fn validate_options(options: VolcOptions) -> Result<VolcOptions, AppError> {
    let resource_id = options.resource_id.trim().to_owned();
    if resource_id.is_empty() || resource_id.chars().count() > 64 {
        return Err(AppError::Configuration(
            "火山引擎资源 ID 不能为空且不能超过 64 个字符".to_owned(),
        ));
    }
    let model = options.model.trim().to_owned();
    if model.is_empty() || model.chars().count() > 64 {
        return Err(AppError::Configuration(
            "火山引擎模型版本不能为空且不能超过 64 个字符".to_owned(),
        ));
    }
    let voice = options.voice.trim().to_owned();
    if voice.is_empty() || voice.chars().count() > 96 {
        return Err(AppError::Configuration(
            "火山引擎音色不能为空且不能超过 96 个字符".to_owned(),
        ));
    }
    if !RATE_RANGE.contains(&options.speech_rate) {
        return Err(AppError::Configuration(
            "火山引擎语速范围为 -50 到 100".to_owned(),
        ));
    }
    if !LOUDNESS_RANGE.contains(&options.loudness_rate) {
        return Err(AppError::Configuration(
            "火山引擎音量范围为 -50 到 100".to_owned(),
        ));
    }
    Ok(VolcOptions {
        resource_id,
        model,
        voice,
        speech_rate: options.speech_rate,
        loudness_rate: options.loudness_rate,
    })
}

pub fn synthesize(
    text: &str,
    options: &VolcOptions,
    api_key: &str,
) -> Result<Synthesis, AppError> {
    let prepared = prepare_for_speech(text);
    if prepared.is_empty() {
        return Err(AppError::Audio("没有可供朗读的正文".to_owned()));
    }
    let mut socket = VolcSocket::connect(options, api_key)?;
    socket.start_connection()?;
    socket.start_session()?;
    let audio = socket.request_audio(&prepared)?;
    socket.finish();
    if audio.is_empty() {
        return Err(AppError::Service("火山引擎未返回语音数据".to_owned()));
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(AppError::Audio("语音音频超过 25 MB，无法播放".to_owned()));
    }
    Ok(Synthesis {
        bytes: audio,
        content_type: AUDIO_CONTENT_TYPE.to_owned(),
    })
}

// ---------------------------------------------------------------------------
// Protocol framing
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
struct ServerMessage {
    message_type: u8,
    event: i32,
    error_code: u32,
    payload: Vec<u8>,
}

fn marshal_client(event: i32, session_id: Option<&str>, payload: &[u8]) -> Vec<u8> {
    let mut frame = vec![
        (1 << 4) | 1,                            // protocol version 1, 4-byte header
        (MSG_FULL_CLIENT_REQUEST << 4) | FLAG_WITH_EVENT,
        SERIALIZATION_JSON << 4,                 // JSON payload, no compression
        0x00,
    ];
    frame.extend_from_slice(&event.to_be_bytes());
    if !CONNECTION_EVENTS.contains(&event) {
        let session = session_id.unwrap_or_default().as_bytes();
        frame.extend_from_slice(&u32_len(session.len()).to_be_bytes());
        frame.extend_from_slice(session);
    }
    frame.extend_from_slice(&u32_len(payload.len()).to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn parse_server_message(data: &[u8]) -> Result<ServerMessage, AppError> {
    if data.len() < 4 {
        return Err(protocol_error());
    }
    let header_size = usize::from(data[0] & 0x0F) * 4;
    if header_size < 4 || data.len() < header_size {
        return Err(protocol_error());
    }
    let message_type = data[1] >> 4;
    let flag = data[1] & 0x0F;
    let mut cursor = header_size;
    let mut event = 0;
    if flag == FLAG_WITH_EVENT {
        event = read_i32(data, &mut cursor)?;
        if !CONNECTION_EVENTS.contains(&event) {
            let size = read_u32(data, &mut cursor)? as usize;
            cursor = cursor.saturating_add(size);
        } else if message_type == MSG_FULL_SERVER_RESPONSE {
            // Connection responses carry a connect id on the wire.
            let size = read_u32(data, &mut cursor)? as usize;
            cursor = cursor.saturating_add(size);
        }
    }
    let mut error_code = 0;
    if message_type == MSG_ERROR_INFORMATION {
        error_code = read_u32(data, &mut cursor)?;
    } else if matches!(flag, FLAG_POSITIVE_SEQ | FLAG_NEGATIVE_SEQ) {
        cursor = cursor.saturating_add(4);
    }
    let payload = if cursor + 4 <= data.len() {
        let size = read_u32(data, &mut cursor)? as usize;
        let end = cursor.saturating_add(size).min(data.len());
        data.get(cursor..end).unwrap_or_default().to_vec()
    } else {
        Vec::new()
    };
    Ok(ServerMessage {
        message_type,
        event,
        error_code,
        payload,
    })
}

fn read_u32(data: &[u8], cursor: &mut usize) -> Result<u32, AppError> {
    let end = cursor.checked_add(4).ok_or_else(protocol_error)?;
    let slice = data.get(*cursor..end).ok_or_else(protocol_error)?;
    *cursor = end;
    Ok(u32::from_be_bytes(
        slice.try_into().map_err(|_| protocol_error())?,
    ))
}

fn read_i32(data: &[u8], cursor: &mut usize) -> Result<i32, AppError> {
    let end = cursor.checked_add(4).ok_or_else(protocol_error)?;
    let slice = data.get(*cursor..end).ok_or_else(protocol_error)?;
    *cursor = end;
    Ok(i32::from_be_bytes(
        slice.try_into().map_err(|_| protocol_error())?,
    ))
}

fn u32_len(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

fn protocol_error() -> AppError {
    AppError::Service("火山引擎返回了无法解析的语音数据".to_owned())
}

// ---------------------------------------------------------------------------
// WebSocket transport
// ---------------------------------------------------------------------------

enum WsFrame {
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

struct VolcSocket {
    stream: BufReader<TlsStream<TcpStream>>,
    session_id: String,
    options: VolcOptions,
}

impl VolcSocket {
    fn connect(options: &VolcOptions, api_key: &str) -> Result<Self, AppError> {
        let tcp = TcpStream::connect((WS_HOST, 443))
            .map_err(|error| AppError::Network(format!("连接火山引擎失败：{error}")))?;
        tcp.set_read_timeout(Some(READ_TIMEOUT))
            .map_err(|error| AppError::Network(format!("设置火山引擎超时失败：{error}")))?;
        tcp.set_write_timeout(Some(WRITE_TIMEOUT))
            .map_err(|error| AppError::Network(format!("设置火山引擎超时失败：{error}")))?;
        let stream = TlsConnector::new()
            .map_err(|error| AppError::Network(format!("初始化火山引擎 TLS 失败：{error}")))?
            .connect(WS_HOST, tcp)
            .map_err(|error| AppError::Network(format!("建立火山引擎 TLS 连接失败：{error}")))?;
        let mut socket = Self {
            stream: BufReader::new(stream),
            session_id: uuid_v4(),
            options: options.clone(),
        };
        socket.upgrade(options, api_key)?;
        Ok(socket)
    }

    fn upgrade(&mut self, options: &VolcOptions, api_key: &str) -> Result<(), AppError> {
        let request_id = uuid_v4();
        let connect_id = uuid_v4();
        // The current Volcengine console authenticates with a single API key; older consoles used
        // App ID + Access Token instead, but Nova targets the API-key flow.
        let request = format!(
            "GET {WS_PATH} HTTP/1.1\r\nHost: {WS_HOST}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {}\r\nX-Api-Key: {api_key}\r\nX-Api-Resource-Id: {}\r\nX-Api-Request-Id: {request_id}\r\nX-Api-Connect-Id: {connect_id}\r\n\r\n",
            websocket_key(),
            options.resource_id,
        );
        self.stream
            .get_mut()
            .write_all(request.as_bytes())
            .and_then(|_| self.stream.get_mut().flush())
            .map_err(|error| AppError::Network(format!("发送火山引擎连接请求失败：{error}")))?;

        let mut status_line = String::new();
        self.stream
            .read_line(&mut status_line)
            .map_err(|error| AppError::Network(format!("读取火山引擎连接响应失败：{error}")))?;
        if !status_line.contains(" 101 ") {
            return Err(handshake_error(&status_line));
        }
        loop {
            let mut header = String::new();
            self.stream
                .read_line(&mut header)
                .map_err(|error| AppError::Network(format!("读取火山引擎响应头失败：{error}")))?;
            if header == "\r\n" || header.is_empty() {
                break;
            }
        }
        Ok(())
    }

    fn send_event(&mut self, event: i32, payload: &[u8]) -> Result<(), AppError> {
        let session_id = (!CONNECTION_EVENTS.contains(&event)).then(|| self.session_id.clone());
        let frame = marshal_client(event, session_id.as_deref(), payload);
        self.send_frame(0x2, &frame)
    }

    fn send_frame(&mut self, opcode: u8, payload: &[u8]) -> Result<(), AppError> {
        let mut frame = vec![0x80 | opcode];
        let payload_length = payload.len();
        if payload_length < 126 {
            frame.push(0x80 | u8::try_from(payload_length).expect("payload length under 126"));
        } else if payload_length <= u16::MAX as usize {
            frame.push(0x80 | 126);
            frame.extend_from_slice(
                &u16::try_from(payload_length)
                    .expect("payload length fits u16")
                    .to_be_bytes(),
            );
        } else {
            frame.push(0x80 | 127);
            frame.extend_from_slice(
                &u64::try_from(payload_length)
                    .expect("usize fits u64")
                    .to_be_bytes(),
            );
        }
        let mask = mask_key();
        frame.extend_from_slice(&mask);
        frame.extend(
            payload
                .iter()
                .enumerate()
                .map(|(index, byte)| byte ^ mask[index % mask.len()]),
        );
        self.stream
            .get_mut()
            .write_all(&frame)
            .and_then(|_| self.stream.get_mut().flush())
            .map_err(|error| AppError::Network(format!("发送火山引擎数据失败：{error}")))
    }

    fn read_frame(&mut self) -> Result<WsFrame, AppError> {
        let mut header = [0_u8; 2];
        self.stream
            .read_exact(&mut header)
            .map_err(read_error)?;
        if header[0] & 0x80 == 0 {
            return Err(AppError::Service("火山引擎返回了不支持的分片帧".to_owned()));
        }
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        let mut length = u64::from(header[1] & 0x7F);
        if length == 126 {
            let mut extended = [0_u8; 2];
            self.stream.read_exact(&mut extended).map_err(read_error)?;
            length = u64::from(u16::from_be_bytes(extended));
        } else if length == 127 {
            let mut extended = [0_u8; 8];
            self.stream.read_exact(&mut extended).map_err(read_error)?;
            length = u64::from_be_bytes(extended);
        }
        if length > MAX_AUDIO_BYTES as u64 + 65_536 {
            return Err(AppError::Service("火山引擎返回的帧过大".to_owned()));
        }
        let mut mask = [0_u8; 4];
        if masked {
            self.stream.read_exact(&mut mask).map_err(read_error)?;
        }
        let mut payload = vec![
            0_u8;
            usize::try_from(length)
                .map_err(|_| AppError::Service("火山引擎帧长度无效".to_owned()))?
        ];
        self.stream.read_exact(&mut payload).map_err(read_error)?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % mask.len()];
            }
        }
        match opcode {
            0x2 => Ok(WsFrame::Binary(payload)),
            0x8 => Ok(WsFrame::Close),
            0x9 => Ok(WsFrame::Ping(payload)),
            0xA => Ok(WsFrame::Pong),
            0x1 => Ok(WsFrame::Binary(payload)),
            _ => Err(AppError::Service("火山引擎返回了未知 WebSocket 帧".to_owned())),
        }
    }

    fn read_message(&mut self) -> Result<ServerMessage, AppError> {
        loop {
            match self.read_frame()? {
                WsFrame::Binary(payload) => return parse_server_message(&payload),
                WsFrame::Ping(payload) => self.send_frame(0xA, &payload)?,
                WsFrame::Pong => {}
                WsFrame::Close => {
                    return Err(AppError::Network("火山引擎已关闭连接".to_owned()));
                }
            }
        }
    }

    fn start_connection(&mut self) -> Result<(), AppError> {
        self.send_event(EVENT_START_CONNECTION, b"{}")?;
        loop {
            let message = self.read_message()?;
            match message.event {
                EVENT_CONNECTION_STARTED => return Ok(()),
                EVENT_CONNECTION_FAILED => return Err(self.failure(&message, "连接被拒绝")),
                _ => {}
            }
        }
    }

    fn start_session(&mut self) -> Result<(), AppError> {
        let payload = self.session_payload(EVENT_START_SESSION, None);
        self.send_event(EVENT_START_SESSION, &payload)?;
        loop {
            let message = self.read_message()?;
            match message.event {
                EVENT_SESSION_STARTED => return Ok(()),
                EVENT_SESSION_FAILED => return Err(self.failure(&message, "会话启动失败")),
                _ => {}
            }
        }
    }

    fn request_audio(&mut self, text: &str) -> Result<Vec<u8>, AppError> {
        // `request_audio` needs the same speaker parameters as the session, so the caller rebuilds
        // them from settings each time. The session payload is captured when the session starts.
        let payload = self.task_payload(text);
        self.send_event(EVENT_TASK_REQUEST, &payload)?;
        self.send_event(EVENT_FINISH_SESSION, b"{}")?;

        let mut audio = Vec::new();
        loop {
            let message = self.read_message()?;
            if message.message_type == MSG_ERROR_INFORMATION {
                return Err(error_information(&message));
            }
            if message.message_type == MSG_AUDIO_ONLY_RESPONSE {
                if !message.payload.is_empty() {
                    audio.extend_from_slice(&message.payload);
                    if audio.len() > MAX_AUDIO_BYTES {
                        return Err(AppError::Audio("语音音频超过 25 MB，无法播放".to_owned()));
                    }
                }
                continue;
            }
            if message.message_type == MSG_FULL_SERVER_RESPONSE {
                match message.event {
                    EVENT_TTS_ENDED | EVENT_SESSION_FINISHED => return Ok(audio),
                    EVENT_SESSION_FAILED => return Err(self.failure(&message, "语音合成失败")),
                    EVENT_TTS_RESPONSE => {}
                    _ => {}
                }
            }
        }
    }

    fn finish(&mut self) {
        let _ = self.send_event(EVENT_FINISH_CONNECTION, b"{}");
    }

    fn session_payload(&self, event: i32, text: Option<&str>) -> Vec<u8> {
        let mut req_params = serde_json::json!({
            "speaker": self.options.voice,
            "model": self.options.model,
            "audio_params": {
                "format": AUDIO_FORMAT,
                "sample_rate": DEFAULT_SAMPLE_RATE,
                "speech_rate": self.options.speech_rate,
                "loudness_rate": self.options.loudness_rate,
            },
        });
        if let Some(text) = text {
            req_params["text"] = serde_json::Value::String(text.to_owned());
        }
        serde_json::json!({
            "user": { "uid": self.session_id },
            "event": event,
            "namespace": "BidirectionalTTS",
            "req_params": req_params,
        })
        .to_string()
        .into_bytes()
    }

    fn task_payload(&self, text: &str) -> Vec<u8> {
        // The session payload is regenerated with the same parameters; only `text` is added.
        self.session_payload(EVENT_TASK_REQUEST, Some(text))
    }

    fn failure(&self, message: &ServerMessage, context: &str) -> AppError {
        let detail = String::from_utf8_lossy(&message.payload);
        let detail = detail.trim();
        let snippet: String = detail.chars().take(200).collect();
        if message.error_code == 0 {
            AppError::Service(format!("{context}：{snippet}"))
        } else {
            AppError::Service(format!("{context}（错误码 {}）：{snippet}", message.error_code))
        }
    }
}

fn error_information(message: &ServerMessage) -> AppError {
    let detail = String::from_utf8_lossy(&message.payload);
    let detail = detail.trim();
    let snippet: String = detail.chars().take(200).collect();
    match message.error_code {
        45000001 | 45000002 => AppError::Configuration(format!("火山引擎参数错误：{snippet}")),
        55000000 => AppError::Service(format!("火山引擎服务端错误：{snippet}")),
        0 => AppError::Service(format!("火山引擎返回错误：{snippet}")),
        code => AppError::Service(format!("火山引擎错误（{code}）：{snippet}")),
    }
}

fn handshake_error(status_line: &str) -> AppError {
    let status = status_line.split_whitespace().nth(1).unwrap_or_default();
    match status {
        "401" | "403" => AppError::Authorization("火山引擎 App ID 或 Access Token 无效".to_owned()),
        "404" => AppError::Configuration("火山引擎接口地址不可用".to_owned()),
        _ => AppError::Network(format!(
            "火山引擎 WebSocket 未升级：{}",
            status_line.trim()
        )),
    }
}

fn read_error(error: std::io::Error) -> AppError {
    AppError::Network(format!("读取火山引擎语音数据失败：{error}"))
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

fn random_bytes(count: usize) -> Vec<u8> {
    let mut source = Vec::new();
    source.extend_from_slice(
        &SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
            .to_be_bytes(),
    );
    source.extend_from_slice(&std::process::id().to_be_bytes());
    source.extend_from_slice(
        &REQUEST_COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .to_be_bytes(),
    );
    Sha256::digest(&source)[..count].to_vec()
}

fn uuid_v4() -> String {
    let bytes = random_bytes(16);
    let mut hex = String::with_capacity(32);
    for byte in bytes {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn websocket_key() -> String {
    BASE64.encode(random_bytes(16))
}

fn mask_key() -> [u8; 4] {
    let bytes = random_bytes(4);
    [bytes[0], bytes[1], bytes[2], bytes[3]]
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MODEL, DEFAULT_RESOURCE_ID, DEFAULT_VOICE, EVENT_START_SESSION, EVENT_TASK_REQUEST,
        FLAG_WITH_EVENT, MSG_FULL_CLIENT_REQUEST, MSG_FULL_SERVER_RESPONSE, VolcOptions,
        marshal_client, parse_server_message, uuid_v4, validate_options,
    };

    fn options() -> VolcOptions {
        VolcOptions {
            resource_id: DEFAULT_RESOURCE_ID.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            voice: DEFAULT_VOICE.to_owned(),
            speech_rate: 0,
            loudness_rate: 0,
        }
    }

    #[test]
    fn accepts_the_pinned_defaults() {
        let validated = validate_options(options()).expect("defaults are valid");
        assert_eq!(validated.resource_id, DEFAULT_RESOURCE_ID);
        assert_eq!(validated.model, DEFAULT_MODEL);
        assert_eq!(validated.voice, DEFAULT_VOICE);
    }

    #[test]
    fn rejects_missing_ids_and_out_of_range_rates() {
        for mutate in [
            |draft: &mut VolcOptions| draft.resource_id = String::new(),
            |draft: &mut VolcOptions| draft.model = String::new(),
            |draft: &mut VolcOptions| draft.voice = String::new(),
            |draft: &mut VolcOptions| draft.speech_rate = 101,
            |draft: &mut VolcOptions| draft.loudness_rate = -51,
        ] {
            let mut draft = options();
            mutate(&mut draft);
            assert!(validate_options(draft).is_err());
        }
    }

    #[test]
    fn uuid_follows_the_canonical_shape() {
        let id = uuid_v4();
        let parts: Vec<&str> = id.split('-').collect();
        assert_eq!(parts.len(), 5);
        assert_eq!(
            parts.iter().map(|part| part.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12]
        );
        assert_eq!(uuid_v4().len(), 36);
        assert_ne!(uuid_v4(), uuid_v4());
    }

    #[test]
    fn marphals_a_session_start_with_session_id() {
        let frame = marshal_client(EVENT_START_SESSION, Some("session-1"), b"{}");
        assert_eq!(frame[0], 0x11);
        assert_eq!(frame[1], (MSG_FULL_CLIENT_REQUEST << 4) | FLAG_WITH_EVENT);
        assert_eq!(frame[2], 0x10);
        // event (4) + session length (4) + session (9) + payload length (4) + payload (2)
        assert_eq!(frame.len(), 4 + 4 + 4 + 9 + 4 + 2);
    }

    #[test]
    fn parses_a_server_event_round_trip() {
        // Header + event + session length/session + payload length/payload.
        let mut frame = vec![
            0x11,
            (MSG_FULL_SERVER_RESPONSE << 4) | FLAG_WITH_EVENT,
            0x10,
            0x00,
        ];
        frame.extend_from_slice(&EVENT_TASK_REQUEST.to_be_bytes());
        frame.extend_from_slice(&9_u32.to_be_bytes());
        frame.extend_from_slice(b"session-1");
        frame.extend_from_slice(&2_u32.to_be_bytes());
        frame.extend_from_slice(b"{}");
        let message = parse_server_message(&frame).expect("frame parses");
        assert_eq!(message.event, EVENT_TASK_REQUEST);
        assert_eq!(message.payload, b"{}");
    }
}
