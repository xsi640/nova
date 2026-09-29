# TASK.md · TTS（Edge / 火山引擎 v3 WebSocket）流式播放交接说明

> 更新时间：2026-09-29
> 仓库：`D:/develop/github/xsi640/nova`（分支 `main`）

## 1. 需求

- 去掉 Piper（离线音质不达标，已移除）。
- TTS 先选方式（Edge / 火山引擎），再配置该方式的参数。
- 火山引擎用 WebSocket 协议；模型版本默认 `seed-tts-2.0-standard`；音色可选，默认小何 2.0。
- 火山鉴权用新控制台的单个 API Key（`X-Api-Key`）。
- **合成与播放改成真流式**（边收边播），并清理无用代码。

## 2. 已完成

### 2.1 后端

- **两个 provider 都提供 `synthesize_stream`**：
  - `edge_tts::synthesize_stream`：每个 `Path:audio` 帧到达即回调；`synthesize` 包装已删除。
  - `volcengine_tts::synthesize_stream`：`request_audio` 每收到一个 `AudioOnlyResponse` 帧即回调；`Synthesis`/`synthesize` 已删除。
- **新命令 `synthesize_speech_stream(database, text, options?, on_event: Channel<TtsStreamEvent>)`**：先发 `Start { content_type }`，再逐个发 `Chunk { data }`（base64），错误通过命令的 `Result` 返回。旧的非流式 `synthesize_speech` / `SpeechSynthesisResult` 已删除。
- **火山 v3 协议**：`wss://openspeech.bytedance.com/api/v3/tts/bidirection`，资源 `seed-tts-2.0`，`req_params.model` 默认 `seed-tts-2.0-standard`，默认音色 `zh_female_xiaohe_uranus_bigtts`，MP3；鉴权头 `X-Api-Key` + `X-Api-Resource-Id`/`X-Api-Request-Id`/`X-Api-Connect-Id`。API Key 存系统凭据存储（引用名 `tts-volcengine`），不入库。
- **数据模型**：`AppSettings` 火山字段 `volc_resource_id`/`volc_model`/`volc_voice`/`volc_speech_rate`/`volc_loudness_rate` + `volc_api_key`(write-only) + `volc_api_key_set`(只读)。迁移 10 加入协议字段；**迁移 11 删除已废弃的 `volc_api_url`/`volc_speed`/`volc_app_id` 列**（SQLite 3.43 支持 `DROP COLUMN`）。

### 2.2 前端

- `src/lib/commands.ts`：`SpeechStreamEvent` + `synthesizeSpeechStream(text, options, onEvent)`（用 `@tauri-apps/api/core` 的 `Channel`）；删除 `synthesizeSpeech` / `SpeechSynthesisResult`。
- 新增 `src/lib/speechPlayer.ts`：`playStreamingSpeech(text, options)` 用 MediaSource（`audio/mpeg`）边收边播，返回 `{ stop(), done }`。
- `src/App.tsx`：`ChatPage.playSpeech` 与设置页试听都改用 `playStreamingSpeech`；删除了按句切分（`splitSpeechSegments`）、整段缓存、`<audio data:...>` 播放与 `speechCacheRef`。

### 2.3 验证

| 检查 | 结果 |
| --- | --- |
| `npm run typecheck` / `npm run check:rust` | ✅ / ✅ 0 warning |
| `npm run test:rust` | ✅ 60 passed / 0 failed / 1 ignored |
| Edge 流式合成（真实联网，`--ignored` 测试） | ✅ |
| MP3 分块入 MSE 播放（无头 Chrome） | ✅ `duration=Infinity`、播放推进到结束、无 error |
| 真实数据库 | ✅ 迁移到 v11，火山默认值正确，死列已删除 |
| 端点/鉴权头实测 | ✅ 假 Key 返回 `401 {"error":"Invalid X-Api-Key"}` |
| `tauri dev` | ✅ 新构建运行 |

### 2.4 附带修复：Windows 弹出 `ms-gamingoverlay`

根因：本机未安装 Xbox Game Bar，且 **Game DVR 自动游戏检测**处于开启状态，任何 GPU/D3D 应用（WebView2 等）都会被当作游戏去激活 Game Bar，Game Bar 缺失就弹「需要新的应用来打开此 ms-gamingoverlay 链接」。仅禁用 Chromium 的 `EnableWindowsGamingInputDataFetcher` **不足以**解决（那只覆盖手柄输入路径）。

已在该用户下关闭 Game DVR（HKCU，可回滚）：
```
HKCU\SOFTWARE\Microsoft\Windows\CurrentVersion\GameDVR : AppCaptureEnabled=0, HistoricalCaptureEnabled=0
HKCU\System\GameConfigStore : GameDVR_Enabled=0
HKCU\SOFTWARE\Microsoft\GameBar : AutoGameModeEnabled=0, ShowStartupPanel=0, UseNexusForGameBarEnabled=0
```
`tauri.conf.json` 仍保留 `additionalBrowserArgs`（含 `EnableWindowsGamingInputDataFetcher`），作为手柄路径的额外保险。

### 2.5 流式对话（边说边生成）

- 后端新增 `send_message_stream(database, content, on_event: Channel<String>) -> ChatExchange`：`request_chat_completion_stream` 用 `stream: true` 调对话 API，逐条解析 SSE（`data: {...}` / `[DONE]`），每段文本增量通过 Channel 推送；结束后仍返回持久化后的 `ChatExchange`（避免事件与命令返回的投递顺序竞争）。若端点忽略 `stream` 返回普通 JSON，则回退解析非流式响应。
- 前端 `submitMessage` 改成：用户消息立即上屏 → 对话增量逐字渲染到一条临时助手消息 → 同时把增量喂给 `createLiveSpeech`（`src/lib/speechPlayer.ts`）：按句边界缓冲，每凑够一句就立即合成并播放，于是回复一边生成一边朗读。完成后用真实消息替换临时消息。
- 弃用并删除旧的 `send_message`（`retry_message` 仍是非流式）。
- 延迟优化：`http_client()` 改为共享客户端（连接池复用）；历史上下文限为 **16 条 / 6000 字符**（`recent_remote_messages`）；设置页新增「快速回复」开关（`chat_fast_mode`，迁移 12），开启时请求带 `reasoning_effort: "low"`；「边说边生成」的开口阈值从 24 字降到 14 字。

## 3. 尚未完成 / 待确认

1. **UI 人工走查**：流式播放（试听、聊天朗读、自动播放、中途停止）未由本人点击验证。
2. **未用真实火山凭证做联网合成**（本机没有 API Key）。
3. **首帧延时量测**：TASK-016 需要按固定文本记录优化前后对比后才能标记完成；TASK-018 的流式对话体验需人工走查。
4. Edge 仍是每个 4096 字节分片新建一次 WebSocket 连接；如需再降延迟可复用单条连接。
5. 遗留 `%APPDATA%\app.nova.companion\piper\`（旧 Piper 运行时/音色）可删。

## 4. 关键实现细节

- v3 帧：4 字节头（`0x11 0x14 0x10 0x00`）+ event(4) + [sessionId 长度+内容] + payload 长度 + payload；音频帧类型 `0b1011`，结束事件 `TTSEnded(359)` 或 `SessionFinished(152)`。
- Tauri Channel 用 JSON + base64 传音频块；`Channel<InvokeResponseBody>` 的 Raw 变体也可用，但当前选 base64 以保留类型化事件。
- 前端 MSE：`MediaSource` + `addSourceBuffer('audio/mpeg')` + `mode="sequence"`；Chromium/WebView2 支持 `audio/mpeg`（已实测 `MediaSource.isTypeSupported('audio/mpeg') === true`）。
- 停止播放：`playStreamingSpeech.stop()` 暂停并 resolve；后端流式请求无法中断，会在后台跑完（前端忽略后续块）。
