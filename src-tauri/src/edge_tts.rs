//! Minimal Rust client for the Microsoft Edge Read Aloud WebSocket protocol.
//!
//! This module deliberately has no Python or sidecar dependency. It only produces an in-memory
//! MP3 payload; callers decide whether to play, cache, or persist it.

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    sync::atomic::{AtomicU64, Ordering},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use native_tls::{TlsConnector, TlsStream};
use serde::Deserialize;
use sha2::{Digest, Sha256};

use crate::error::AppError;

const HOST: &str = "speech.platform.bing.com";
const TRUSTED_CLIENT_TOKEN: &str = "6A5AA1D4EAFF4E9FB37E23D68491D6F4";
const CHROMIUM_FULL_VERSION: &str = "143.0.3650.75";
const MAX_CHUNK_BYTES: usize = 4_096;
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

pub const DEFAULT_VOICE: &str = "zh-CN-XiaoxiaoNeural";
pub const DEFAULT_RATE: i32 = -5;
pub const DEFAULT_PITCH: i32 = 0;
pub const DEFAULT_VOLUME: i32 = 0;
pub const RATE_RANGE: std::ops::RangeInclusive<i32> = -50..=100;
pub const PITCH_RANGE: std::ops::RangeInclusive<i32> = -50..=50;
pub const VOLUME_RANGE: std::ops::RangeInclusive<i32> = -50..=50;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TtsOptions {
    pub voice: String,
    pub rate: i32,
    pub pitch: i32,
    pub volume: i32,
}

impl Default for TtsOptions {
    fn default() -> Self {
        Self {
            voice: DEFAULT_VOICE.to_owned(),
            rate: DEFAULT_RATE,
            pitch: DEFAULT_PITCH,
            volume: DEFAULT_VOLUME,
        }
    }
}

pub fn validate_voice(value: String) -> Result<String, AppError> {
    let voice = value.trim().to_owned();
    if voice.is_empty() || voice.chars().count() > 96 {
        return Err(AppError::Configuration(
            "Edge TTS 音色不能为空且不能超过 96 个字符".to_owned(),
        ));
    }
    if !voice
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(AppError::Configuration(
            "Edge TTS 音色只能包含字母、数字和连字符".to_owned(),
        ));
    }
    Ok(voice)
}

pub fn validate_options(options: TtsOptions) -> Result<TtsOptions, AppError> {
    let voice = validate_voice(options.voice)?;
    if !RATE_RANGE.contains(&options.rate) {
        return Err(AppError::Configuration(
            "Edge TTS 语速范围为 -50% 到 +100%".to_owned(),
        ));
    }
    if !PITCH_RANGE.contains(&options.pitch) {
        return Err(AppError::Configuration(
            "Edge TTS 音调范围为 -50Hz 到 +50Hz".to_owned(),
        ));
    }
    if !VOLUME_RANGE.contains(&options.volume) {
        return Err(AppError::Configuration(
            "Edge TTS 音量范围为 -50% 到 +50%".to_owned(),
        ));
    }
    Ok(TtsOptions { voice, ..options })
}

pub fn synthesize(text: &str, options: &TtsOptions) -> Result<Vec<u8>, AppError> {
    let options = validate_options(options.clone())?;
    let prepared = prepare_for_speech(text);
    if prepared.is_empty() {
        return Err(AppError::Audio("没有可供朗读的正文".to_owned()));
    }
    let mut audio = Vec::new();
    for chunk in split_text(&prepared) {
        let chunk_audio = synthesize_chunk(&chunk, &options)?;
        if audio.len().saturating_add(chunk_audio.len()) > MAX_AUDIO_BYTES {
            return Err(AppError::Audio(
                "Edge TTS 音频超过 25 MB，无法播放".to_owned(),
            ));
        }
        audio.extend_from_slice(&chunk_audio);
    }
    if audio.is_empty() {
        return Err(AppError::Audio("Edge TTS 返回了空音频".to_owned()));
    }
    Ok(audio)
}

fn synthesize_chunk(text: &str, options: &TtsOptions) -> Result<Vec<u8>, AppError> {
    let mut socket = connect()?;
    let timestamp = edge_timestamp()?;
    socket.send_text(&speech_config(&timestamp))?;
    socket.send_text(&ssml_request(&timestamp, text, options))?;

    let mut audio = Vec::new();
    loop {
        match socket.read_frame()? {
            WebSocketFrame::Text(message) => {
                if message.contains("Path:turn.end") {
                    break;
                }
                if message.contains("Path:response") && message.contains("status") {
                    return Err(AppError::Audio(format!("Edge TTS 服务拒绝请求：{message}")));
                }
            }
            WebSocketFrame::Binary(message) => {
                if message.len() < 2 {
                    return Err(AppError::Audio("Edge TTS 返回的音频帧不完整".to_owned()));
                }
                let header_length = usize::from(u16::from_be_bytes([message[0], message[1]]));
                let audio_start = 2usize.saturating_add(header_length);
                if audio_start > message.len() {
                    return Err(AppError::Audio(
                        "Edge TTS 返回的音频帧头长度无效".to_owned(),
                    ));
                }
                let headers = &message[2..audio_start];
                if headers
                    .windows(b"Path:audio".len())
                    .any(|window| window == b"Path:audio")
                {
                    audio.extend_from_slice(&message[audio_start..]);
                    if audio.len() > MAX_AUDIO_BYTES {
                        return Err(AppError::Audio(
                            "Edge TTS 音频超过 25 MB，无法播放".to_owned(),
                        ));
                    }
                }
            }
            WebSocketFrame::Ping(payload) => socket.send_control(0xA, &payload)?,
            WebSocketFrame::Close => break,
            WebSocketFrame::Pong => {}
        }
    }
    if audio.is_empty() {
        return Err(AppError::Audio(
            "Edge TTS 未返回音频，请检查网络或稍后重试".to_owned(),
        ));
    }
    Ok(audio)
}

fn connect() -> Result<EdgeWebSocket, AppError> {
    let tcp = TcpStream::connect((HOST, 443))
        .map_err(|error| AppError::Network(format!("连接 Edge TTS 失败：{error}")))?;
    tcp.set_read_timeout(Some(Duration::from_secs(60)))
        .map_err(|error| AppError::Network(format!("设置 Edge TTS 超时失败：{error}")))?;
    tcp.set_write_timeout(Some(Duration::from_secs(15)))
        .map_err(|error| AppError::Network(format!("设置 Edge TTS 超时失败：{error}")))?;
    let stream = TlsConnector::new()
        .map_err(|error| AppError::Network(format!("初始化 Edge TTS TLS 失败：{error}")))?
        .connect(HOST, tcp)
        .map_err(|error| AppError::Network(format!("建立 Edge TTS TLS 连接失败：{error}")))?;
    let mut socket = EdgeWebSocket::new(stream);
    socket.upgrade()?;
    Ok(socket)
}

struct EdgeWebSocket {
    stream: BufReader<TlsStream<TcpStream>>,
}

enum WebSocketFrame {
    Text(String),
    Binary(Vec<u8>),
    Ping(Vec<u8>),
    Pong,
    Close,
}

impl EdgeWebSocket {
    fn new(stream: TlsStream<TcpStream>) -> Self {
        Self {
            stream: BufReader::new(stream),
        }
    }

    fn upgrade(&mut self) -> Result<(), AppError> {
        let connection_id = identifier(16);
        let sec_ms_gec = sec_ms_gec()?;
        let websocket_key = websocket_key();
        let request = format!(
            "GET /consumer/speech/synthesize/readaloud/edge/v1?TrustedClientToken={TRUSTED_CLIENT_TOKEN}&ConnectionId={connection_id}&Sec-MS-GEC={sec_ms_gec}&Sec-MS-GEC-Version=1-{CHROMIUM_FULL_VERSION} HTTP/1.1\r\nHost: {HOST}\r\nConnection: Upgrade\r\nUpgrade: websocket\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: {websocket_key}\r\nPragma: no-cache\r\nCache-Control: no-cache\r\nOrigin: chrome-extension://jdiccldimpdaibmpdkjnbmckianbfold\r\nUser-Agent: Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/143.0.0.0 Safari/537.36 Edg/143.0.0.0\r\nCookie: muid={} ;\r\n\r\n",
            identifier(32)
        );
        self.stream
            .get_mut()
            .write_all(request.as_bytes())
            .and_then(|_| self.stream.get_mut().flush())
            .map_err(|error| AppError::Network(format!("发送 Edge TTS 连接请求失败：{error}")))?;

        let mut status_line = String::new();
        self.stream
            .read_line(&mut status_line)
            .map_err(|error| AppError::Network(format!("读取 Edge TTS 连接响应失败：{error}")))?;
        if !status_line.contains(" 101 ") {
            return Err(AppError::Network(format!(
                "Edge TTS WebSocket 未升级：{}",
                status_line.trim()
            )));
        }
        loop {
            let mut header = String::new();
            self.stream
                .read_line(&mut header)
                .map_err(|error| AppError::Network(format!("读取 Edge TTS 响应头失败：{error}")))?;
            if header == "\r\n" || header.is_empty() {
                break;
            }
        }
        Ok(())
    }

    fn send_text(&mut self, message: &str) -> Result<(), AppError> {
        self.send_control(0x1, message.as_bytes())
    }

    fn send_control(&mut self, opcode: u8, payload: &[u8]) -> Result<(), AppError> {
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
            .map_err(|error| {
                AppError::Network(format!("发送 Edge TTS WebSocket 数据失败：{error}"))
            })
    }

    fn read_frame(&mut self) -> Result<WebSocketFrame, AppError> {
        let mut header = [0_u8; 2];
        self.stream
            .read_exact(&mut header)
            .map_err(|error| AppError::Network(format!("读取 Edge TTS 音频失败：{error}")))?;
        if header[0] & 0x80 == 0 {
            return Err(AppError::Audio("Edge TTS 返回了不支持的分片帧".to_owned()));
        }
        let opcode = header[0] & 0x0F;
        let masked = header[1] & 0x80 != 0;
        let mut length = u64::from(header[1] & 0x7F);
        if length == 126 {
            let mut extended = [0_u8; 2];
            self.stream
                .read_exact(&mut extended)
                .map_err(read_frame_error)?;
            length = u64::from(u16::from_be_bytes(extended));
        } else if length == 127 {
            let mut extended = [0_u8; 8];
            self.stream
                .read_exact(&mut extended)
                .map_err(read_frame_error)?;
            length = u64::from_be_bytes(extended);
        }
        if length > MAX_AUDIO_BYTES as u64 + 16_384 {
            return Err(AppError::Audio("Edge TTS 返回的帧过大".to_owned()));
        }
        let mut mask = [0_u8; 4];
        if masked {
            self.stream
                .read_exact(&mut mask)
                .map_err(read_frame_error)?;
        }
        let mut payload = vec![
            0_u8;
            usize::try_from(length)
                .map_err(|_| AppError::Audio("Edge TTS 帧长度无效".to_owned()))?
        ];
        self.stream
            .read_exact(&mut payload)
            .map_err(read_frame_error)?;
        if masked {
            for (index, byte) in payload.iter_mut().enumerate() {
                *byte ^= mask[index % mask.len()];
            }
        }
        match opcode {
            0x1 => Ok(WebSocketFrame::Text(
                String::from_utf8_lossy(&payload).into_owned(),
            )),
            0x2 => Ok(WebSocketFrame::Binary(payload)),
            0x8 => Ok(WebSocketFrame::Close),
            0x9 => Ok(WebSocketFrame::Ping(payload)),
            0xA => Ok(WebSocketFrame::Pong),
            _ => Err(AppError::Audio(
                "Edge TTS 返回了未知 WebSocket 帧".to_owned(),
            )),
        }
    }
}

fn read_frame_error(error: std::io::Error) -> AppError {
    AppError::Network(format!("读取 Edge TTS 音频失败：{error}"))
}

fn speech_config(timestamp: &str) -> String {
    format!(
        "X-Timestamp:{timestamp}\r\nContent-Type:application/json; charset=utf-8\r\nPath:speech.config\r\n\r\n{{\"context\":{{\"synthesis\":{{\"audio\":{{\"metadataoptions\":{{\"sentenceBoundaryEnabled\":\"false\",\"wordBoundaryEnabled\":\"false\"}},\"outputFormat\":\"audio-24khz-48kbitrate-mono-mp3\"}}}}}}}}\r\n"
    )
}

fn ssml_request(timestamp: &str, text: &str, options: &TtsOptions) -> String {
    format!(
        "X-RequestId:{}\r\nContent-Type:application/ssml+xml\r\nX-Timestamp:{timestamp}Z\r\nPath:ssml\r\n\r\n<speak version='1.0' xmlns='http://www.w3.org/2001/10/synthesis' xml:lang='zh-CN'><voice name='{}'><prosody pitch='{:+}Hz' rate='{:+}%' volume='{:+}%'>{text}</prosody></voice></speak>",
        identifier(16),
        options.voice,
        options.pitch,
        options.rate,
        options.volume,
    )
}

fn prepare_for_speech(text: &str) -> String {
    let mut prepared = String::new();
    let mut in_code_fence = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        let cleaned_line = strip_markdown_line(line);
        if !cleaned_line.trim().is_empty() {
            append_normalized(&mut prepared, "\n");
            append_normalized(&mut prepared, &cleaned_line);
        }
    }
    prepared.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn strip_markdown_line(line: &str) -> String {
    let characters = line.chars().collect::<Vec<_>>();
    let mut output = String::new();
    let mut index = 0;
    while characters
        .get(index)
        .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    }
    while characters
        .get(index)
        .is_some_and(|character| matches!(character, '#' | '>'))
    {
        index += 1;
    }
    if characters
        .get(index)
        .is_some_and(|character| matches!(character, '-' | '+' | '*'))
        && characters
            .get(index + 1)
            .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    } else {
        let number_start = index;
        while characters
            .get(index)
            .is_some_and(|character| character.is_ascii_digit())
        {
            index += 1;
        }
        if index > number_start
            && characters.get(index) == Some(&'.')
            && characters
                .get(index + 1)
                .is_some_and(|character| character.is_whitespace())
        {
            index += 1;
        } else {
            index = number_start;
        }
    }
    while characters
        .get(index)
        .is_some_and(|character| character.is_whitespace())
    {
        index += 1;
    }
    while index < characters.len() {
        let character = characters[index];
        let is_image = character == '!' && characters.get(index + 1) == Some(&'[');
        let is_link = character == '[';
        if is_image || is_link {
            let label_start = if is_image { index + 2 } else { index + 1 };
            if let Some(label_end) = characters[label_start..]
                .iter()
                .position(|item| *item == ']')
            {
                let label_end = label_start + label_end;
                if characters.get(label_end + 1) == Some(&'(') {
                    if let Some(url_end) = characters[label_end + 2..]
                        .iter()
                        .position(|item| *item == ')')
                    {
                        let label = characters[label_start..label_end]
                            .iter()
                            .collect::<String>();
                        append_normalized(&mut output, &strip_markdown_line(&label));
                        index = label_end + 3 + url_end;
                        continue;
                    }
                }
            }
        }
        let remaining = characters[index..].iter().collect::<String>();
        if (remaining.starts_with("https://") || remaining.starts_with("http://"))
            && (index == 0 || characters[index - 1].is_whitespace())
        {
            while index < characters.len() && !characters[index].is_whitespace() {
                index += 1;
            }
            continue;
        }
        if character != '`' && is_speech_character(character) {
            output.push(character);
        }
        index += 1;
    }
    output
}

fn append_normalized(target: &mut String, value: &str) {
    for character in value.chars() {
        if character.is_whitespace() {
            if !target.ends_with(' ') && !target.is_empty() {
                target.push(' ');
            }
        } else {
            target.push(character);
        }
    }
}

fn is_speech_character(character: char) -> bool {
    character.is_alphanumeric()
        || character.is_whitespace()
        || matches!(
            character,
            '，' | '。'
                | '！'
                | '？'
                | '；'
                | '：'
                | '、'
                | ','
                | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '…'
                | '—'
                | '-'
                | '('
                | ')'
                | '（'
                | '）'
                | '“'
                | '”'
                | '‘'
                | '’'
                | '「'
                | '」'
                | '《'
                | '》'
        )
}

fn split_text(text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        let escaped = escape_character(character);
        if !current.is_empty() && current.len() + escaped.len() > MAX_CHUNK_BYTES {
            chunks.push(current);
            current = String::new();
        }
        current.push_str(&escaped);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn escape_character(character: char) -> String {
    match character {
        '&' => "&amp;".to_owned(),
        '<' => "&lt;".to_owned(),
        '>' => "&gt;".to_owned(),
        '\'' => "&apos;".to_owned(),
        '"' => "&quot;".to_owned(),
        '\u{0000}'..='\u{0008}' | '\u{000B}'..='\u{000C}' | '\u{000E}'..='\u{001F}' => {
            " ".to_owned()
        }
        _ => character.to_string(),
    }
}

fn sec_ms_gec() -> Result<String, AppError> {
    let unix_seconds = unix_seconds()?;
    let windows_seconds = unix_seconds.saturating_add(11_644_473_600);
    let rounded_seconds = windows_seconds - windows_seconds % 300;
    Ok(sha256_hex(format!(
        "{}{}",
        rounded_seconds.saturating_mul(10_000_000),
        TRUSTED_CLIENT_TOKEN
    )))
}

fn edge_timestamp() -> Result<String, AppError> {
    let seconds = unix_seconds()?;
    let days = i64::try_from(seconds / 86_400)
        .map_err(|_| AppError::internal("Edge TTS 时间超出范围".to_owned()))?;
    let time = seconds % 86_400;
    let (year, month, day) = civil_from_days(days);
    let weekday = ["Thu", "Fri", "Sat", "Sun", "Mon", "Tue", "Wed"]
        [usize::try_from(days.rem_euclid(7)).expect("weekday is in range")];
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    Ok(format!(
        "{weekday} {} {day:02} {year} {:02}:{:02}:{:02} GMT+0000 (Coordinated Universal Time)",
        months[usize::try_from(month - 1).expect("month is in range")],
        time / 3_600,
        (time % 3_600) / 60,
        time % 60
    ))
}

fn unix_seconds() -> Result<u64, AppError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| AppError::internal(format!("系统时间无效：{error}")))
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = (if shifted >= 0 {
        shifted
    } else {
        shifted - 146_096
    }) / 146_097;
    let day_of_era = shifted - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    (year + i64::from(month <= 2), month as u32, day as u32)
}

fn identifier(bytes: usize) -> String {
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
    sha256_hex_bytes(&source)[..bytes * 2].to_owned()
}

fn websocket_key() -> String {
    let hexadecimal = identifier(16);
    let bytes = hexadecimal
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            u8::from_str_radix(std::str::from_utf8(pair).expect("hex is UTF-8"), 16)
                .expect("identifier is hex")
        })
        .collect::<Vec<_>>();
    BASE64.encode(bytes)
}

fn mask_key() -> [u8; 4] {
    let hexadecimal = identifier(4);
    let mut mask = [0_u8; 4];
    for (index, pair) in hexadecimal.as_bytes().chunks_exact(2).enumerate() {
        mask[index] = u8::from_str_radix(std::str::from_utf8(pair).expect("hex is UTF-8"), 16)
            .expect("identifier is hex");
    }
    mask
}

fn sha256_hex(value: String) -> String {
    sha256_hex_bytes(value.as_bytes())
}

fn sha256_hex_bytes(value: &[u8]) -> String {
    Sha256::digest(value)
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_PITCH, DEFAULT_RATE, DEFAULT_VOICE, DEFAULT_VOLUME, TtsOptions, edge_timestamp,
        prepare_for_speech, sec_ms_gec, split_text, validate_options, validate_voice,
    };

    #[test]
    fn accepts_official_voice_names_and_rejects_command_text() {
        assert_eq!(
            validate_voice(DEFAULT_VOICE.to_owned()).expect("voice is accepted"),
            DEFAULT_VOICE
        );
        assert!(validate_voice("voice; rm -rf /".to_owned()).is_err());
    }

    #[test]
    fn validates_and_formats_tts_options() {
        let options = validate_options(TtsOptions {
            voice: DEFAULT_VOICE.to_owned(),
            rate: DEFAULT_RATE,
            pitch: DEFAULT_PITCH,
            volume: DEFAULT_VOLUME,
        })
        .expect("default options are valid");
        let request = super::ssml_request("timestamp", "你好", &options);
        assert!(request.contains("voice name='zh-CN-XiaoxiaoNeural'"));
        assert!(request.contains("pitch='+0Hz' rate='-5%' volume='+0%'"));
        assert!(
            validate_options(TtsOptions {
                rate: 101,
                ..options
            })
            .is_err()
        );
    }

    #[test]
    fn splits_escaped_text_without_breaking_the_protocol_limit() {
        let chunks = split_text(&"&".repeat(1_100));
        assert_eq!(chunks.len(), 2);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 4_096));
        assert_eq!(chunks.concat(), "&amp;".repeat(1_100));
    }

    #[test]
    fn prepares_markdown_without_speaking_symbols_or_urls() {
        let prepared = prepare_for_speech(
            "## **Nova** 🎉\n\n请查看 [官方文档](https://example.com/docs)。\n`代码` 和 @#$%",
        );
        assert_eq!(prepared, "Nova 请查看 官方文档。 代码 和");
    }

    #[test]
    fn removes_markdown_list_prefixes_but_keeps_normal_hyphens() {
        let prepared = prepare_for_speech("- 第一项\n2. 第二项\n> 引用\n温度 -5 度");
        assert_eq!(prepared, "第一项 第二项 引用 温度 -5 度");
    }

    #[test]
    fn protocol_timestamp_and_token_have_expected_shapes() {
        assert!(
            edge_timestamp()
                .expect("timestamp is available")
                .contains("GMT+0000")
        );
        let token = sec_ms_gec().expect("token is available");
        assert_eq!(token.len(), 64);
        assert!(
            token
                .chars()
                .all(|character| character.is_ascii_uppercase() || character.is_ascii_digit())
        );
    }

    #[test]
    #[ignore = "uses the Edge TTS online service"]
    fn synthesizes_a_short_mp3_via_the_rust_websocket_client() {
        let audio = super::synthesize("Nova 语音服务测试。", &TtsOptions::default())
            .expect("Edge TTS returns audio");
        assert!(audio.len() > 1_000);
        assert!(
            audio.starts_with(b"ID3")
                || audio.first() == Some(&0xFF)
                    && audio
                        .get(1)
                        .is_some_and(|byte| byte & 0b1110_0000 == 0b1110_0000)
        );
    }
}
