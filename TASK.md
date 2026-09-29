# TASK.md · TTS「先选方式再配置」（Edge / 火山引擎 v3 WebSocket）交接说明

> 更新时间：2026-09-29
> 仓库：`D:/develop/github/xsi640/nova`（分支 `main`）

## 1. 需求

- **去掉 Piper**（离线方案音质不达标，已整体移除）。
- TTS **先选方式（Edge / 火山引擎），再配置该方式的参数**。
- 火山引擎使用 **WebSocket TTS 协议**；**模型版本默认 `seed-tts-2.0-standard`**；**音色可选，默认小何 2.0**。

## 2. 已完成

### 2.1 后端

- **移除 Piper**：删除 `src-tauri/src/piper_tts.rs`、`mod piper_tts`、命令 `get_piper_status` / `install_piper_voice`、`nova:piper-progress` 事件、`app_data_root`，以及 `Cargo.toml` 的 `zip`（`tempfile` 退回 dev-dependencies）。`speech_text.rs` 保留，Edge/火山共用文本清洗。
- **重写 `src-tauri/src/volcengine_tts.rs`：原生 v3 双向流式 WebSocket**
  - 端点 `wss://openspeech.bytedance.com/api/v3/tts/bidirection`；资源 ID 默认 `seed-tts-2.0`；模型默认 `seed-tts-2.0-standard`（可选 `seed-tts-2.0-expressive`）；音色默认 `zh_female_xiaohe_uranus_bigtts`（小何 2.0）；输出 MP3。
  - 自实现握手 + 二进制帧（`native-tls` + 掩码帧），协议事件序列 `StartConnection → StartSession → TaskRequest → FinishSession → FinishConnection`；`req_params` 含 `speaker`/`model`/`audio_params{format,sample_rate,speech_rate,loudness_rate}`，`namespace=BidirectionalTTS`。
  - 单测：默认值校验、非法参数、UUID 形状、marshal/parse 往返，共 5 个。
- **`commands/mod.rs`**：`SpeechProvider { Edge, Volcengine }`；`save_settings` 按方式校验；`get_settings` 回填 `volc_api_key_set`；`synthesize_speech(database, text, options?)`，`SpeechOptions` 全可选（provider/voice/rate/pitch/volume/resourceId/model/speechRate/loudnessRate/apiKey），未提供时回退已保存设置。
- **密钥**：火山 **API Key** 存系统凭据存储（引用名 `tts-volcengine`），**不进数据库**。
- **`infrastructure/database.rs`**：`AppSettings` 的火山字段为 `volc_resource_id`/`volc_model`/`volc_voice`/`volc_speech_rate`/`volc_loudness_rate` + `volc_api_key`(write-only) + `volc_api_key_set`(只读)。`DEFAULT_TTS_PROVIDER="edge"`。迁移 9（网关时代，已废弃的 `volc_api_url`/`volc_speed` 列保留但不读取）+ **迁移 10**（新增本协议的列并把 `volc_model`/`volc_voice` 设为新默认）。

### 2.2 前端

- `src/lib/commands.ts`：`TtsProvider = "edge" | "volcengine"`；`AppSettings`/`SpeechOptions` 同步新字段；`synthesizeSpeech(text, options?)`。
- `src/App.tsx`：默认 `edge`；`TtsSettingsCard` 方式切换——Edge：音色/语速/声调/音量；火山：API Key / 模型版本（标准/表现力）/ 音色（带 2.0 建议，默认小何 2.0）/ 语速 / 音量；试听传对应 `SpeechOptions`。

### 2.3 验证

| 检查 | 结果 |
| --- | --- |
| `npm run typecheck` | ✅ |
| `npm run check:rust` | ✅ 0 warning |
| `npm run test:rust` | ✅ 60 passed / 0 failed / 1 ignored |
| 真实数据库 | ✅ 迁移到 v10；`tts_provider=edge`，`volc_resource_id=seed-tts-2.0`，`volc_model=seed-tts-2.0-standard`，`volc_voice=zh_female_xiaohe_uranus_bigtts` |
| 端点/鉴权头实测 | ✅ 对 `wss://openspeech.bytedance.com/api/v3/tts/bidirection` 发真实握手（假凭证）返回 `401`，证明 URL 与 `X-Api-*` 头被识别 |
| `tauri dev` 启动 | ✅ 新构建运行 |

## 3. 尚未完成 / 待确认

1. **UI 人工走查**：两种方式的试听、聊天朗读与自动播放未由本人点击验证。
2. **未用真实凭证做联网合成**（本机没有火山 API Key）。
3. **首帧延时（TASK-016）**：目前后端是「一次性接收整段 MP3 再返回」，未把 WebSocket 音频流边收边推给前端；如需更低延迟，可后续做成流式回传。
4. 遗留 `%APPDATA%\app.nova.companion\piper\`（旧 Piper 运行时/音色）可删。

### 2.4 附带修复：Windows 弹出 `ms-gamingoverlay` 提示

现象：使用应用时 Windows 反复弹“需要新的应用来打开此 ms-gamingoverlay 链接”。

原因：本机未安装 Xbox Game Bar，而 WebView2 运行时 120 默认启用 Chromium 的 `EnableWindowsGamingInputDataFetcher`（内部调用 `Windows.Gaming.Input`），会被 Windows 当作游戏而激活 Game Bar，Game Bar 缺失就变成这个弹窗。

修复：`src-tauri/tauri.conf.json` 两个窗口都加 `additionalBrowserArgs`（必须带上 wry 默认项）：
`--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection,EnableWindowsGamingInputDataFetcher,WindowsGamingInputDataFetcher`
已实测 nova 的 `msedgewebview2.exe` 进程带上了该参数。若仍弹出，可在系统层关闭 Game DVR / 重装 Game Bar。

## 4. 使用方式

设置页 → 语音播放 → 选「火山引擎」→ 填火山「语音技术」控制台的 **API Key** → 选音色（默认小何 2.0）→ 试听 → 保存。资源 ID 默认 `seed-tts-2.0`。

## 5. 关键实现细节

- v3 协议帧：4 字节头（`0x11 0x14 0x10 0x00` 表示 version1/JSON/无压缩/WithEvent）+ event(4) + [sessionId 长度+内容] + payload 长度 + payload；服务端音频帧类型 `0b1011`（AudioOnlyResponse），结束事件 `TTSEnded(359)` 或 `SessionFinished(152)`。
- 鉴权：新控制台用单个 **API Key**，请求头发 `X-Api-Key`；另发 `X-Api-Resource-Id`/`X-Api-Request-Id`/`X-Api-Connect-Id`。实测假 Key 返回 `401 {"error":"Invalid X-Api-Key"}`。
- 客户端为直连（不走代理），因为火山域名在国内可直连；`reqwest` 的代理环境变量不影响本模块。
