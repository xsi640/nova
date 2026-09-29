//! Online Mandarin speech synthesis through Volcengine's Doubao voice service.
//!
//! Volcengine exposes an OpenAI-compatible `POST /v1/audio/speech` endpoint (the "边缘大模型网关"
//! gateway), which is the simplest way to reach the `doubao-tts` big-model voices. Nova only needs
//! a Bearer access key: the signed WebSocket protocol used by the native console is deliberately
//! not implemented. The access key lives in the OS credential store, never in the database.

use std::time::Duration;

use serde_json::json;

use crate::{error::AppError, speech_text::prepare_for_speech};

/// Credential-store reference for the Volcengine gateway access key.
pub const SECRET_REFERENCE: &str = "tts-volcengine";

pub const DEFAULT_API_URL: &str = "https://ai-gateway.vei.volces.com/v1/audio/speech";
pub const DEFAULT_MODEL: &str = "doubao-tts";
pub const DEFAULT_VOICE: &str = "zh_female_shuangkuaisisi_moon_bigtts";
pub const DEFAULT_SPEED: f64 = 1.0;
pub const SPEED_RANGE: std::ops::RangeInclusive<f64> = 0.25..=4.0;

const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq)]
pub struct VolcOptions {
    pub api_url: String,
    pub model: String,
    pub voice: String,
    pub speed: f64,
}

#[derive(Debug)]
pub struct Synthesis {
    pub bytes: Vec<u8>,
    pub content_type: String,
}

pub fn validate_options(options: VolcOptions) -> Result<VolcOptions, AppError> {
    let api_url = options.api_url.trim().trim_end_matches('/').to_owned();
    let parsed = reqwest::Url::parse(&api_url)
        .map_err(|_| AppError::Configuration("火山引擎接口地址不是有效 URL".to_owned()))?;
    if !matches!(parsed.scheme(), "http" | "https") || parsed.host().is_none() {
        return Err(AppError::Configuration(
            "火山引擎接口地址必须使用 http 或 https".to_owned(),
        ));
    }
    let model = options.model.trim().to_owned();
    if model.is_empty() || model.chars().count() > 120 {
        return Err(AppError::Configuration(
            "火山引擎模型不能为空且不能超过 120 个字符".to_owned(),
        ));
    }
    let voice = options.voice.trim().to_owned();
    if voice.is_empty() || voice.chars().count() > 96 {
        return Err(AppError::Configuration(
            "火山引擎音色不能为空且不能超过 96 个字符".to_owned(),
        ));
    }
    if !options.speed.is_finite() || !SPEED_RANGE.contains(&options.speed) {
        return Err(AppError::Configuration(
            "火山引擎语速范围为 0.25 到 4.0".to_owned(),
        ));
    }
    Ok(VolcOptions {
        api_url,
        model,
        voice,
        speed: options.speed,
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
    let client = reqwest::blocking::Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|error| AppError::internal(format!("无法创建语音合成客户端：{error}")))?;
    let response = client
        .post(&options.api_url)
        .bearer_auth(api_key)
        .json(&json!({
            "model": options.model,
            "input": prepared,
            "voice": options.voice,
            "speed": options.speed,
        }))
        .send()
        .map_err(|error| AppError::Network(format!("火山引擎语音合成请求失败：{error}")))?;

    let status = response.status();
    if !status.is_success() {
        let detail = response.text().unwrap_or_default();
        return Err(failure_error(status.as_u16(), &detail));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or(value).trim().to_owned())
        .filter(|value| value.starts_with("audio/"))
        .unwrap_or_else(|| "audio/mpeg".to_owned());
    let bytes = response
        .bytes()
        .map_err(|error| AppError::Service(format!("无法读取火山引擎语音数据：{error}")))?
        .to_vec();
    if bytes.len() < 128 {
        return Err(AppError::Service("火山引擎返回了空的语音数据".to_owned()));
    }
    if bytes.len() > MAX_AUDIO_BYTES {
        return Err(AppError::Audio("语音音频超过 25 MB，无法播放".to_owned()));
    }
    Ok(Synthesis { bytes, content_type })
}

fn failure_error(status: u16, detail: &str) -> AppError {
    let detail = detail.trim();
    let snippet: String = detail.chars().take(200).collect();
    match status {
        401 | 403 => AppError::Authorization("火山引擎访问密钥无效或没有权限".to_owned()),
        404 => AppError::Configuration("火山引擎接口地址或模型不存在".to_owned()),
        429 => AppError::Service("火山引擎请求过于频繁，请稍后再试".to_owned()),
        400..=499 => AppError::Configuration(format!(
            "火山引擎语音合成参数被拒绝（{status}）：{snippet}"
        )),
        _ => AppError::Service(format!("火山引擎语音合成失败（{status}）：{snippet}")),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_API_URL, DEFAULT_MODEL, DEFAULT_VOICE, SPEED_RANGE, VolcOptions, validate_options,
    };

    fn options() -> VolcOptions {
        VolcOptions {
            api_url: DEFAULT_API_URL.to_owned(),
            model: DEFAULT_MODEL.to_owned(),
            voice: DEFAULT_VOICE.to_owned(),
            speed: 1.0,
        }
    }

    #[test]
    fn accepts_the_pinned_defaults() {
        let validated = validate_options(options()).expect("defaults are valid");
        assert_eq!(validated.api_url, DEFAULT_API_URL);
        assert_eq!(validated.model, DEFAULT_MODEL);
        assert_eq!(validated.voice, DEFAULT_VOICE);
    }

    #[test]
    fn trims_a_trailing_slash_and_rejects_bad_urls() {
        let mut draft = options();
        draft.api_url = format!("{DEFAULT_API_URL}/");
        assert_eq!(
            validate_options(draft).expect("url is accepted").api_url,
            DEFAULT_API_URL
        );

        let mut draft = options();
        draft.api_url = "not-a-url".to_owned();
        assert!(validate_options(draft).is_err());
    }

    #[test]
    fn rejects_empty_model_voice_and_out_of_range_speed() {
        for mutate in [
            |draft: &mut VolcOptions| draft.model = "  ".to_owned(),
            |draft: &mut VolcOptions| draft.voice = String::new(),
            |draft: &mut VolcOptions| draft.speed = 0.1,
            |draft: &mut VolcOptions| draft.speed = 5.0,
        ] {
            let mut draft = options();
            mutate(&mut draft);
            assert!(validate_options(draft).is_err());
        }
        assert!(!SPEED_RANGE.contains(&0.1));
    }
}