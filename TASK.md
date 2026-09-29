# TASK.md · TTS 支持「先选方式再配置」（Edge / 火山引擎）交接说明

> 更新时间：2026-09-29
> 仓库：`D:/develop/github/xsi640/nova`（分支 `main`）

## 1. 本次需求

- **去掉 Piper**（离线方案音质不达标，已整体移除）。
- **TTS 增加「方式」选择：Edge / 火山引擎**，先选方式，再对所选方式配置参数。

## 2. 已完成

### 2.1 后端

- **移除 Piper**：删除 `src-tauri/src/piper_tts.rs`、`mod piper_tts`、命令 `get_piper_status` / `install_piper_voice`、进度事件 `nova:piper-progress`、`app_data_root`，以及 `Cargo.toml` 的 `zip` 依赖（`tempfile` 退回 dev-dependencies）。`speech_text.rs` 保留，Edge 仍共用文本清洗。
- **新增 `src-tauri/src/volcengine_tts.rs`**：火山引擎豆包语音，OpenAI 兼容 `POST /v1/audio/speech`（默认 `https://ai-gateway.vei.volces.com/v1/audio/speech`，模型 `doubao-tts`，默认音色 `zh_female_shuangkuaisisi_moon_bigtts`，语速 0.25–4.0）。含 `validate_options` 与 3 个单测；按 HTTP 状态映射到配置/授权/服务错误。
- **`commands/mod.rs`**：
  - `SpeechProvider { Edge, Volcengine }`；`save_settings` 按方式分别校验并规范化；`get_settings` 回填 `volc_api_key_set`。
  - 火山引擎访问密钥写入系统凭据存储（引用名 `tts-volcengine`），**不进数据库**；`get_settings` 只返回是否已保存，`save_settings` 仅在传入非空密钥时写入。
  - `synthesize_speech` 改为 `(database, text, options?)`，`SpeechOptions` 为全可选覆盖（provider / voice / rate / pitch / volume / apiUrl / model / speed / apiKey）；未提供时回退到已保存设置。试听时可带草稿值（含未保存的 Key）。
- **`infrastructure/database.rs`**：`AppSettings` 新增 `volc_api_url`、`volc_model`、`volc_voice`、`volc_speed`（非敏感，入库）与 `volc_api_key`（write-only，`skip_serializing`）、`volc_api_key_set`（只读）。`DEFAULT_TTS_PROVIDER = "edge"`。迁移 8 默认改为 `edge`；**迁移 9** 新增 4 个火山列，并把历史 `tts_provider='piper'` 的记录改回 `edge` + Edge 默认音色。

### 2.2 前端

- `src/lib/commands.ts`：`TtsProvider = "edge" | "volcengine"`；`AppSettings` 增加火山字段；新增 `SpeechOptions`；`synthesizeSpeech(text, options?)`；移除全部 Piper 类型与命令。
- `src/App.tsx`：`defaultSettings` 默认 `edge`；`TtsSettingsCard` 改为「Edge 在线 / 火山引擎（豆包）」方式切换——Edge 显示音色/语速/声调/音量；火山显示访问密钥/接口地址/模型/音色（带常用 bigtts 建议）/语速；试听按当前方式传入对应 `SpeechOptions`。
- `src/styles.css`：清理不再使用的 `.tts-download`。

### 2.3 验证

| 检查 | 结果 |
| --- | --- |
| `npm run typecheck` | ✅ 通过 |
| `npm run check:rust` | ✅ 0 warning |
| `npm run test:rust` | ✅ 58 passed / 0 failed / 1 ignored |
| 真实数据库 `%APPDATA%\app.nova.companion\nova.db` | ✅ 迁移到 v9，`tts_provider` 已由 `piper` 改为 `edge`，火山列写入默认值 |
| `npm run tauri dev` 启动 | ✅ `target\debug\nova.exe` 正常启动（新构建） |

## 3. 尚未完成 / 待确认

1. **UI 人工走查**：两种方式的试听、聊天朗读与自动播放未由本人点击验证。
2. **火山引擎访问密钥需要用户自行开通**：在火山引擎「边缘大模型网关」开通 Doubao 语音合成、创建访问密钥后填入设置页；密钥存系统凭据存储。
3. **火山引擎方式未做真实联网合成验证**（本机无火山访问密钥）。
4. **TASK-016 首帧延时**：Edge（WebSocket 流式）与火山（一次性返回整段音频）需按同一口径分别量测；若火山首帧偏慢，可后续接入其流式大模型接口。
5. 遗留目录 `%APPDATA%\app.nova.companion\piper\`（旧 Piper 运行时与音色）已不再使用，可手动删除。
6. 文档已同步：`docs/tts-options-research.md`、`docs/tech-architecture.md`、`docs/tasks.md`（TASK-017 改为「语音方式选择 + 火山引擎豆包语音」）；`docs/xiaozhi-tts-research.md` 记录小智 AI 的 TTS 选型（火山豆包 bigtts + DashScope CosyVoice）。

## 4. 关键实现细节

- 火山走的是 **OpenAI 兼容网关**而不是原生控制台协议：原生 v1（appid/access_token/cluster）拿不到大模型音色，v3 是带签名的 WebSocket、集成成本高。若必须用控制台 appid/token，需要再加一个适配器。
- `SpeechOptions.apiKey` 允许试听时临时传入密钥；**聊天朗读不带 options，后端只从系统凭据存储读取已保存密钥**。
- 迁移 9 的 `UPDATE` 用 `CASE WHEN tts_provider='piper'` 同时改写 provider 与 voice（SQLite 同一 UPDATE 的各表达式按原始行取值）。
