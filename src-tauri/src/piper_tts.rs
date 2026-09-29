//! Offline Mandarin speech synthesis through the Piper voice engine.
//!
//! Piper is used as a sidecar executable instead of a linked library, so Nova never compiles
//! native code on the user's machine and the model files stay replaceable. The runtime and the
//! voice models are downloaded on demand into the application data directory, verified against a
//! pinned SHA-256 digest, and then invoked locally for synthesis.
//!
//! Layout under the application data directory:
//!
//! ```text
//! piper/runtime/piper.exe
//! piper/runtime/espeak-ng-data/...
//! piper/voices/zh_CN-huayan-medium.onnx
//! piper/voices/zh_CN-huayan-medium.onnx.json
//! ```

use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{error::AppError, speech_text};

/// The runtime release that the pinned digests below belong to.
pub const RUNTIME_VERSION: &str = "2023.11.14-2";
pub const RUNTIME_PROVIDER: &str = "rhasspy/piper";
pub const DEFAULT_VOICE: &str = "zh_CN-huayan-medium";
pub const RATE_RANGE: std::ops::RangeInclusive<i32> = -50..=100;

/// Offline Piper synthesis needs a prebuilt runtime; only Windows x64 has one today.
pub const SUPPORTED: bool = cfg!(all(target_os = "windows", target_arch = "x86_64"));

const RUNTIME_DIRECTORY: &str = "piper/runtime";
const VOICE_DIRECTORY: &str = "piper/voices";
const STAGING_DIRECTORY: &str = "piper/runtime-staging";
const MAX_AUDIO_BYTES: usize = 25 * 1024 * 1024;
const PROGRESS_STEP_BYTES: u64 = 512 * 1024;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, Copy)]
pub struct RemoteFile {
    pub url: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
}

impl RemoteFile {
    fn file_name(self) -> &'static str {
        self.url.rsplit('/').next().unwrap_or("download")
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VoicePack {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub license: &'static str,
    pub model: RemoteFile,
    pub config: RemoteFile,
}

impl VoicePack {
    pub fn download_bytes(self) -> u64 {
        self.model.bytes + self.config.bytes
    }
}

/// Pinned Piper runtime for Windows x64 (MIT licensed, published by the Piper project).
const RUNTIME: RemoteFile = RemoteFile {
    url: "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip",
    bytes: 22_477_236,
    sha256: "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea",
};

/// Chinese voices that were verified against the pinned runtime.
///
/// `zh_CN-chaowen-medium` and `zh_CN-xiao_ya-medium` are deliberately absent: both need the
/// Python piper 1.4+ phonemizer and fail in the prebuilt runtime with
/// `"ai" is not a single codepoint`.
pub const VOICES: &[VoicePack] = &[
    VoicePack {
        id: "zh_CN-huayan-medium",
        label: "华研 · 标准音质",
        note: "简体中文女声，22.05 kHz，首次约 60 MB",
        license: "运行时 MIT；模型数据集许可未标注",
        model: RemoteFile {
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/zh/zh_CN/huayan/medium/zh_CN-huayan-medium.onnx",
            bytes: 63_201_294,
            sha256: "9929917bf8cabb26fd528ea44d3a6699c11e87317a14765312420be230be0f3d",
        },
        config: RemoteFile {
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/zh/zh_CN/huayan/medium/zh_CN-huayan-medium.onnx.json",
            bytes: 4_822,
            sha256: "d521dc45504a8ccc99e325822b35946dd701840bfb07e3dbb31a40929ed6a82b",
        },
    },
    VoicePack {
        id: "zh_CN-huayan-x_low",
        label: "华研 · 小体积",
        note: "简体中文女声，16 kHz，首次约 20 MB",
        license: "运行时 MIT；模型数据集许可未标注",
        model: RemoteFile {
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/zh/zh_CN/huayan/x_low/zh_CN-huayan-x_low.onnx",
            bytes: 20_628_813,
            sha256: "d30b143fac66d821a1285aa013295adf5cd129d3cc11d70334e51c7b20662c37",
        },
        config: RemoteFile {
            url: "https://huggingface.co/rhasspy/piper-voices/resolve/main/zh/zh_CN/huayan/x_low/zh_CN-huayan-x_low.onnx.json",
            bytes: 4_164,
            sha256: "5521dcb09adf68a9bee289032f7f5af18d29bff020953429b5d223ec1f881816",
        },
    },
];

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallPhase {
    Runtime,
    Voice,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallProgress {
    pub phase: InstallPhase,
    pub received_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VoiceStatus {
    pub id: &'static str,
    pub label: &'static str,
    pub note: &'static str,
    pub license: &'static str,
    pub installed: bool,
    pub download_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PiperStatus {
    pub supported: bool,
    pub runtime_ready: bool,
    pub runtime_version: &'static str,
    pub runtime_provider: &'static str,
    pub runtime_url: &'static str,
    pub runtime_bytes: u64,
    pub default_voice: &'static str,
    pub voices: Vec<VoiceStatus>,
    /// Bytes still to download before the currently selected voice can be used.
    pub pending_bytes: u64,
}

pub fn validate_voice(voice: &str) -> Result<&'static VoicePack, AppError> {
    let voice = voice.trim();
    VOICES.iter().find(|pack| pack.id == voice).ok_or_else(|| {
        AppError::Configuration(format!("不支持离线语音「{voice}」，请在语音设置中选择已下载的语音"))
    })
}

pub fn validate_rate(rate: i32) -> Result<(), AppError> {
    if !RATE_RANGE.contains(&rate) {
        return Err(AppError::Configuration(
            "离线语音语速范围为 -50% 到 +100%".to_owned(),
        ));
    }
    Ok(())
}

pub fn status(root: &Path, selected_voice: &str) -> PiperStatus {
    let runtime_ready = SUPPORTED && runtime_installed(root);
    let voices = VOICES
        .iter()
        .map(|pack| VoiceStatus {
            id: pack.id,
            label: pack.label,
            note: pack.note,
            license: pack.license,
            installed: voice_installed(root, pack),
            download_bytes: pack.download_bytes(),
        })
        .collect::<Vec<_>>();
    let pending_bytes = pending_bytes(&voices, selected_voice, runtime_ready);
    PiperStatus {
        supported: SUPPORTED,
        runtime_ready,
        runtime_version: RUNTIME_VERSION,
        runtime_provider: RUNTIME_PROVIDER,
        runtime_url: RUNTIME.url,
        runtime_bytes: RUNTIME.bytes,
        default_voice: DEFAULT_VOICE,
        voices,
        pending_bytes,
    }
}

fn pending_bytes(voices: &[VoiceStatus], selected_voice: &str, runtime_ready: bool) -> u64 {
    let runtime_missing = if runtime_ready { 0 } else { RUNTIME.bytes };
    let voice_missing = voices
        .iter()
        .find(|voice| voice.id == selected_voice)
        .filter(|voice| !voice.installed)
        .map_or(0, |voice| voice.download_bytes);
    runtime_missing + voice_missing
}

/// Downloads whatever is still missing so that `voice_id` can be synthesized offline.
pub fn install(
    root: &Path,
    voice_id: &str,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), AppError> {
    if !SUPPORTED {
        return Err(AppError::Configuration(
            "离线 Piper 语音目前仅支持 Windows x64".to_owned(),
        ));
    }
    let voice = validate_voice(voice_id)?;
    if !runtime_installed(root) {
        install_runtime(root, progress)?;
    }
    install_voice(root, voice, progress)
}

pub fn synthesize(root: &Path, text: &str, voice_id: &str, rate: i32) -> Result<Vec<u8>, AppError> {
    if !SUPPORTED {
        return Err(AppError::Configuration(
            "离线 Piper 语音目前仅支持 Windows x64，请在语音设置中改用在线音色".to_owned(),
        ));
    }
    validate_rate(rate)?;
    let voice = validate_voice(voice_id)?;
    if !runtime_installed(root) {
        return Err(AppError::Configuration(
            "离线语音运行时尚未就绪，请先在语音设置中下载".to_owned(),
        ));
    }
    if !voice_installed(root, voice) {
        return Err(AppError::Configuration(format!(
            "语音包「{}」尚未下载，请先在语音设置中下载",
            voice.label
        )));
    }
    let prepared = speech_text::prepare_for_speech(text);
    if prepared.is_empty() {
        return Err(AppError::Audio("没有可供朗读的正文".to_owned()));
    }

    let directory = tempfile::tempdir()
        .map_err(|error| AppError::internal(format!("无法创建语音临时目录：{error}")))?;
    let output = directory.path().join("speech.wav");
    let runtime = root.join(RUNTIME_DIRECTORY);
    let mut command = Command::new(runtime.join(executable_name()));
    command
        .arg("--model")
        .arg(voice_path(root, voice, "onnx"))
        .arg("--config")
        .arg(voice_path(root, voice, "onnx.json"))
        .arg("--output_file")
        .arg(&output)
        .arg("--espeak_data")
        .arg(runtime.join("espeak-ng-data"))
        .arg("--length_scale")
        .arg(length_scale(rate))
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|error| AppError::Audio(format!("无法启动离线语音引擎：{error}")))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(prepared.as_bytes())
            .map_err(|error| AppError::Audio(format!("无法向离线语音引擎发送文本：{error}")))?;
    }
    let result = child
        .wait_with_output()
        .map_err(|error| AppError::Audio(format!("离线语音合成失败：{error}")))?;
    if !result.status.success() {
        return Err(AppError::Audio(format!(
            "离线语音合成失败：{}",
            failure_message(&result.stderr)
        )));
    }

    let audio = fs::read(&output)
        .map_err(|error| AppError::Audio(format!("无法读取离线语音音频：{error}")))?;
    if audio.len() < 44 || !audio.starts_with(b"RIFF") {
        return Err(AppError::Audio("离线语音引擎返回了无效的音频".to_owned()));
    }
    if audio.len() > MAX_AUDIO_BYTES {
        return Err(AppError::Audio("离线语音音频超过 25 MB，无法播放".to_owned()));
    }
    Ok(audio)
}

/// Piper speaks `--length_scale` times slower than its training pace.
fn length_scale(rate: i32) -> String {
    let scale = (1.0 / (1.0 + f64::from(rate) / 100.0)).clamp(0.5, 2.0);
    format!("{scale:.2}")
}

fn failure_message(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let message = text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .next_back()
        .unwrap_or("没有错误详情");
    if message.chars().count() > 240 {
        format!("{}…", message.chars().take(240).collect::<String>())
    } else {
        message.to_owned()
    }
}

fn executable_name() -> &'static str {
    if cfg!(windows) {
        "piper.exe"
    } else {
        "piper"
    }
}

fn runtime_installed(root: &Path) -> bool {
    root.join(RUNTIME_DIRECTORY).join(executable_name()).is_file()
}

fn voice_installed(root: &Path, voice: &VoicePack) -> bool {
    [("onnx", voice.model), ("onnx.json", voice.config)]
        .iter()
        .all(|(extension, file)| {
            fs::metadata(voice_path(root, voice, extension))
                .is_ok_and(|metadata| metadata.len() == file.bytes)
        })
}

fn voice_path(root: &Path, voice: &VoicePack, extension: &str) -> PathBuf {
    root.join(VOICE_DIRECTORY).join(voice_file_name(voice, extension))
}

fn voice_file_name(voice: &VoicePack, extension: &str) -> String {
    format!("{}.{extension}", voice.id)
}

/// Partially downloaded files keep the final name plus a `.part` marker so that a cancelled
/// download can never be mistaken for an installed voice.
fn voice_partial_path(root: &Path, voice: &VoicePack, extension: &str) -> PathBuf {
    root.join(VOICE_DIRECTORY)
        .join(format!("{}.{extension}.part", voice.id))
}

fn install_runtime(
    root: &Path,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), AppError> {
    let archive = root.join("piper").join("runtime-download.zip.part");
    download(RUNTIME, &archive, InstallPhase::Runtime, progress)?;

    let staging = root.join(STAGING_DIRECTORY);
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|error| {
            AppError::internal(format!("无法清理上一次的语音运行时目录：{error}"))
        })?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| AppError::internal(format!("无法创建语音运行时目录：{error}")))?;
    let extracted = extract_archive(&archive, &staging);
    let _ = fs::remove_file(&archive);
    extracted?;
    if !staging.join(executable_name()).is_file() {
        let _ = fs::remove_dir_all(&staging);
        return Err(AppError::internal(
            "下载的语音运行时缺少 piper 可执行文件".to_owned(),
        ));
    }

    let runtime = root.join(RUNTIME_DIRECTORY);
    if runtime.exists() {
        fs::remove_dir_all(&runtime)
            .map_err(|error| AppError::internal(format!("无法替换旧的语音运行时：{error}")))?;
    }
    fs::create_dir_all(
        runtime
            .parent()
            .ok_or_else(|| AppError::internal("语音运行时目录无效".to_owned()))?,
    )
    .map_err(|error| AppError::internal(format!("无法创建语音运行时目录：{error}")))?;
    fs::rename(&staging, &runtime)
        .or_else(|_| promote_directory(&staging, &runtime))
        .map_err(|error| AppError::internal(format!("无法启用语音运行时：{error}")))?;
    Ok(())
}

/// Enables the staged runtime. Renaming a directory that contains sub-directories can be denied
/// on some Windows volumes (observed on this machine's D: drive), so fall back to a recursive
/// copy followed by removing the staging directory.
fn promote_directory(staging: &Path, runtime: &Path) -> std::io::Result<()> {
    copy_directory(staging, runtime)?;
    fs::remove_dir_all(staging)
}

fn copy_directory(from: &Path, to: &Path) -> std::io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// The release archive wraps every entry in a `piper/` directory, which is removed here so the
/// staged directory matches the final `piper/runtime` layout.
fn extract_archive(archive: &Path, staging: &Path) -> Result<(), AppError> {
    let file = fs::File::open(archive)
        .map_err(|error| AppError::internal(format!("无法读取语音运行时压缩包：{error}")))?;
    let mut zip = zip::ZipArchive::new(file)
        .map_err(|error| AppError::internal(format!("语音运行时压缩包无效：{error}")))?;
    let mut extracted = false;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| AppError::internal(format!("无法读取压缩包条目：{error}")))?;
        let Some(path) = entry.enclosed_name() else {
            return Err(AppError::internal(
                "语音运行时压缩包包含不安全的路径".to_owned(),
            ));
        };
        let relative = path
            .components()
            .skip_while(|component| component.as_os_str() == "piper")
            .collect::<PathBuf>();
        if relative.as_os_str().is_empty() {
            continue;
        }
        let destination = staging.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&destination)
                .map_err(|error| AppError::internal(format!("无法创建目录：{error}")))?;
            continue;
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| AppError::internal(format!("无法创建目录：{error}")))?;
        }
        let mut target = fs::File::create(&destination)
            .map_err(|error| AppError::internal(format!("无法写入文件：{error}")))?;
        std::io::copy(&mut entry, &mut target)
            .map_err(|error| AppError::internal(format!("无法解压语音运行时：{error}")))?;
        extracted = true;
    }
    if !extracted {
        return Err(AppError::internal("语音运行时压缩包为空".to_owned()));
    }
    Ok(())
}

fn install_voice(
    root: &Path,
    voice: &VoicePack,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), AppError> {
    if voice_installed(root, voice) {
        return Ok(());
    }
    fs::create_dir_all(root.join(VOICE_DIRECTORY)).map_err(|error| {
        AppError::internal(format!("无法创建语音包目录：{error}"))
    })?;
    for (extension, file) in [("onnx", voice.model), ("onnx.json", voice.config)] {
        let destination = voice_path(root, voice, extension);
        if fs::metadata(&destination).is_ok_and(|metadata| metadata.len() == file.bytes) {
            continue;
        }
        let temporary = voice_partial_path(root, voice, extension);
        download(file, &temporary, InstallPhase::Voice, progress)?;
        let _ = fs::remove_file(&destination);
        fs::rename(&temporary, &destination)
            .map_err(|error| AppError::internal(format!("无法启用语音包：{error}")))?;
    }
    Ok(())
}

fn download(
    file: RemoteFile,
    destination: &Path,
    phase: InstallPhase,
    progress: &mut dyn FnMut(InstallProgress),
) -> Result<(), AppError> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| AppError::internal(format!("无法创建下载目录：{error}")))?;
    }
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .timeout(Duration::from_secs(1_800))
        .build()
        .map_err(|error| AppError::internal(format!("无法创建下载客户端：{error}")))?;
    let mut response = client
        .get(file.url)
        .send()
        .map_err(|error| AppError::Network(format!("下载 {} 失败：{error}", file.file_name())))?;
    if !response.status().is_success() {
        return Err(AppError::Network(format!(
            "下载 {} 失败：服务器返回 {}",
            file.file_name(),
            response.status().as_u16()
        )));
    }

    let mut target = fs::File::create(destination)
        .map_err(|error| AppError::internal(format!("无法写入下载文件：{error}")))?;
    let mut digest = Sha256::new();
    let mut received = 0_u64;
    let mut reported = 0_u64;
    let mut buffer = vec![0_u8; 64 * 1024];
    progress(InstallProgress {
        phase,
        received_bytes: 0,
        total_bytes: file.bytes,
    });
    loop {
        let read = response
            .read(&mut buffer)
            .map_err(|error| AppError::Network(format!("下载 {} 中断：{error}", file.file_name())))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
        target
            .write_all(&buffer[..read])
            .map_err(|error| AppError::internal(format!("无法写入下载文件：{error}")))?;
        received += read as u64;
        if received.saturating_sub(reported) >= PROGRESS_STEP_BYTES {
            reported = received;
            progress(InstallProgress {
                phase,
                received_bytes: received,
                total_bytes: file.bytes,
            });
        }
    }
    drop(target);

    let verify = || -> Result<(), AppError> {
        if received != file.bytes {
            return Err(AppError::Network(format!(
                "下载 {} 的大小不正确（期望 {} 字节，实际 {received} 字节）",
                file.file_name(),
                file.bytes
            )));
        }
        let actual = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual != file.sha256 {
            return Err(AppError::Network(format!(
                "下载 {} 的校验值不匹配，已丢弃",
                file.file_name()
            )));
        }
        Ok(())
    };
    if let Err(error) = verify() {
        let _ = fs::remove_file(destination);
        return Err(error);
    }
    progress(InstallProgress {
        phase,
        received_bytes: received,
        total_bytes: file.bytes,
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use tempfile::{TempDir, tempdir};

    use super::{DEFAULT_VOICE, InstallPhase, RUNTIME_DIRECTORY, VOICES, status, synthesize};

    fn empty_root() -> TempDir {
        tempdir().expect("temporary directory")
    }

    #[test]
    fn every_voice_carries_a_pinned_digest_and_size() {
        for voice in VOICES {
            let model_name = super::voice_file_name(voice, "onnx");
            let config_name = super::voice_file_name(voice, "onnx.json");
            assert_eq!(voice.model.url.rsplit('/').next(), Some(model_name.as_str()));
            assert_eq!(voice.config.url.rsplit('/').next(), Some(config_name.as_str()));
            assert_eq!(voice.model.sha256.len(), 64);
            assert_eq!(voice.config.sha256.len(), 64);
            assert!(voice.model.bytes > 1_000_000);
        }
        assert!(VOICES.iter().any(|voice| voice.id == DEFAULT_VOICE));
    }

    #[test]
    fn reports_missing_runtime_and_voices_before_installation() {
        let root = empty_root();
        let status = status(root.path(), DEFAULT_VOICE);

        assert!(!status.runtime_ready);
        assert!(status.voices.iter().all(|voice| !voice.installed));
        assert_eq!(
            status.pending_bytes,
            super::RUNTIME.bytes
                + VOICES
                    .iter()
                    .find(|voice| voice.id == DEFAULT_VOICE)
                    .expect("default voice is pinned")
                    .download_bytes()
        );
    }

    #[test]
    fn copies_a_staged_directory_tree_when_rename_is_unavailable() {
        let root = empty_root();
        let staging = root.path().join("staging");
        std::fs::create_dir_all(staging.join("espeak-ng-data")).expect("nested staging directory");
        std::fs::write(staging.join("piper.exe"), b"stub").expect("runtime stub");
        std::fs::write(staging.join("espeak-ng-data").join("voice"), b"data").expect("nested file");
        let runtime = root.path().join("runtime");

        super::copy_directory(&staging, &runtime).expect("directory tree is copied");

        assert!(runtime.join("piper.exe").is_file());
        assert!(runtime.join("espeak-ng-data").join("voice").is_file());
    }

    #[test]
    fn reports_a_ready_runtime_and_installed_voice() {
        let root = empty_root();
        let runtime = root.path().join(RUNTIME_DIRECTORY);
        std::fs::create_dir_all(&runtime).expect("runtime directory");
        std::fs::write(runtime.join(super::executable_name()), b"stub").expect("runtime stub");
        std::fs::create_dir_all(root.path().join(super::VOICE_DIRECTORY)).expect("voice directory");
        let voice = VOICES
            .iter()
            .find(|voice| voice.id == DEFAULT_VOICE)
            .expect("default voice is pinned");
        for (extension, file) in [("onnx", voice.model), ("onnx.json", voice.config)] {
            std::fs::write(
                super::voice_path(root.path(), voice, extension),
                vec![0_u8; usize::try_from(file.bytes).expect("size fits usize")],
            )
            .expect("voice file");
        }

        let status = status(root.path(), DEFAULT_VOICE);
        assert!(status.runtime_ready);
        assert_eq!(status.pending_bytes, 0);
        assert!(
            status
                .voices
                .iter()
                .any(|voice| voice.id == DEFAULT_VOICE && voice.installed)
        );
    }

    #[test]
    fn rejects_unknown_voices_and_rates() {
        assert!(super::validate_voice("zh_CN-unknown-medium").is_err());
        assert!(super::validate_voice(DEFAULT_VOICE).is_ok());
        assert!(super::validate_rate(101).is_err());
        assert!(super::validate_rate(-5).is_ok());
    }

    #[test]
    fn maps_rate_to_the_piper_length_scale() {
        assert_eq!(super::length_scale(0), "1.00");
        assert_eq!(super::length_scale(100), "0.50");
        assert_eq!(super::length_scale(-50), "2.00");
        // Values outside the search range are clamped instead of producing invalid audio.
        assert_eq!(super::length_scale(-90), "2.00");
    }

    #[test]
    fn refuses_to_synthesize_before_the_voice_pack_is_downloaded() {
        let root = empty_root();
        let error = synthesize(root.path(), "你好", DEFAULT_VOICE, 0);
        if super::SUPPORTED {
            assert!(error.is_err());
        }
    }

    #[test]
    fn refuses_to_extract_archives_into_a_missing_staging_directory() {
        let root = empty_root();
        let archive = root.path().join("missing.zip");
        assert!(super::extract_archive(&archive, &root.path().join("staging")).is_err());
    }

    #[test]
    fn install_phases_have_distinct_identities() {
        assert_ne!(
            serde_json::to_string(&InstallPhase::Runtime).expect("serializable phase"),
            serde_json::to_string(&InstallPhase::Voice).expect("serializable phase")
        );
    }

    #[test]
    #[ignore = "downloads the Piper runtime and voice model, then synthesizes offline"]
    fn installs_and_synthesizes_offline() {
        // `NOVA_PIPER_TEST_ROOT` lets a repeated run reuse one download; otherwise the runtime and
        // the voice model go into a temporary directory that is intentionally kept for inspection.
        let root = std::env::var_os("NOVA_PIPER_TEST_ROOT").map_or_else(
            || empty_root().keep(),
            std::path::PathBuf::from,
        );
        super::install(&root, DEFAULT_VOICE, &mut |_| {}).expect("installation succeeds");
        let audio = synthesize(&root, "你好，这是离线语音测试。", DEFAULT_VOICE, 0)
            .expect("offline synthesis succeeds");
        assert!(audio.starts_with(b"RIFF"));
        assert!(audio.len() > 1_000);
    }
}
