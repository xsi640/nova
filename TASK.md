# TASK.md · TTS 更换为 Piper 离线中文语音（交接说明）

> 交接时间：2026-09-28
> 仓库：`D:/develop/github/xsi640/nova`（分支 `main`）
> 状态：**P0 已于 2026-09-29 完成**（前端接线、K-1 修复、K-2 加固、提交），详见文末「十、进展更新（2026-09-29）」。P1（文档更新、时延量测）仍待做。

---

## 十、进展更新（2026-09-29）

本轮完成第 7 节 **P0 全部条目**：

1. **K-1 已修**：`reports_a_ready_runtime_and_installed_voice` 现在先 `create_dir_all` 语音目录。
2. **前端已接线**：
   - `src/lib/commands.ts`：`AppSettings.ttsProvider`、`PiperStatus`/`PiperVoiceStatus`/`PiperInstallProgress` 类型，新增 `getPiperStatus()`、`installPiperVoice(voiceId)`，`synthesizeSpeech(text, options?, provider?)`。
   - `src/App.tsx`：`defaultSettings` 默认 `piper` + `zh_CN-huayan-medium`；`TtsSettingsCard` 增加离线/在线切换、离线音色列表（已下载/需下载与体积）、下载按钮 + `nova:piper-progress` 进度、平台不支持提示；piper 模式隐藏声调/音量；试听显式传 provider。
   - `src/styles.css`：新增 `.tts-hint` / `.tts-download`。
3. **K-2 已加固**：`promote_directory` 在 `fs::rename` 失败时回退为递归复制 + 删除 staging；新增单测 `copies_a_staged_directory_tree_when_rename_is_unavailable`。
4. **`AppSettings.tts_provider` 增加 `#[serde(default = "default_tts_provider")]`**，旧前端/旧数据缺字段不再整体反序列化失败。

验证结果：

| 检查 | 结果 |
| --- | --- |
| `npm run typecheck` | ✅ 通过 |
| `npm run check:rust` | ✅ 0 warning |
| `npm run test:rust` | ✅ **64 passed / 0 failed / 2 ignored** |
| 离线端到端（复用 C: 已下载运行时+音色，`NOVA_PIPER_TEST_ROOT`） | ✅ 通过 |
| 真实 app data 路径（`%APPDATA%\app.nova.companion`）离线合成 | ✅ 通过 |
| `npm run tauri dev` 启动 | ✅ `target\debug\nova.exe` 正常启动 |

说明：已把已验证的 `piper/runtime` 与 `piper/voices/zh_CN-huayan-medium.onnx(.json)` 从 `%TEMP%\piper-verify-c` 复制进 `%APPDATA%\app.nova.companion\piper\`，本机可直接试听/朗读，无需再下载 85 MB。

仍未完成：**UI 人工走查**（下载进度条、试听、聊天朗读与自动播放的实际交互未由本人点击验证）；**TASK-016 的 Piper 时延重测**；K-4（huayan 数据集许可 Unknown）仍需产品确认。

P1 状态：**文档更新已完成**（`docs/tts-options-research.md`、`docs/tech-architecture.md`、`docs/tasks.md` 已同步为 Piper 离线默认 + Edge 在线备选，新增 TASK-017）。

---

## 0. 接手前必做（否则换电脑后工作丢失）

当前所有改动都在工作区，**没有 commit、没有 push**：

```bash
cd <repo>
git status          # 见「修改过的主要文件」一节的文件列表
git add -A src-tauri TASK.md
git commit -m "feat: switch speech synthesis to offline Piper with Chinese voices"
git push            # 或整目录拷贝到另一台电脑
```

`TASK.md` 本身也是未跟踪文件，需要一并提交。未提交则换机后只剩旧代码（`edge_tts` 单提供商版本）。

---

## 1. 当前任务目标

用户要求：**把应用的 TTS 部分从 Edge TTS（在线）换成 Piper TTS（本地离线），并使用中文语音模型试试。**

隐含要求（从项目文档推导）：本地优先、不在用户机器上编译原生依赖、模型可单独下载、许可与来源可追溯（见 `docs/tts-options-research.md` 的既有结论与约束）。

---

## 2. 已完成的工作

### 2.1 调研（已完成，结论已固化进代码常量）

| 项目 | 结论 |
| --- | --- |
| Piper 运行时 | 使用 `rhasspy/piper` release **`2023.11.14-2`** 的 `piper_windows_amd64.zip`（22,477,236 字节，sha256 `f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea`，MIT） |
| 更高版本运行时 | `OHF-Voice/piper1-gpl` ≥ 1.3（GPL-3.0）**只发布 Python wheel，没有独立 Windows 可执行文件**；wheel 内只有 `piper/espeakbridge.pyd`（需要 CPython），不能当 sidecar 用，故不采用 |
| 可用中文音色 | `zh_CN-huayan-medium`（63,201,294 B，sha256 `9929917bf8cabb26fd528ea44d3a6699c11e87317a14765312420be230be0f3d`）、`zh_CN-huayan-x_low`（20,628,813 B，sha256 `d30b143fac66d821a1285aa013295adf5cd129d3cc11d70334e51c7b20662c37`）；配置文件 sha256 分别为 `d521dc45…`、`5521dcb0…`（见 `piper_tts.rs` 常量） |
| 不采用的中文音色 | `zh_CN-chaowen-medium`、`zh_CN-xiao_ya-medium`（后者数据集为 Data Baker **非商用**；两者都需要 Python piper 1.4+ 的音素化，用本项目 pin 的运行时实测报错 `"ai" is not a single codepoint (ids=30,)`）。已排除并在代码注释里写明原因 |
| 实测性能 | 模型加载约 0.4 s，RTF 0.06–0.13（22.05 kHz 单声道 16 bit WAV 输出），离线合成正常 |

实测样本（可直接播放核对音质）：`C:\Users\ZCG04000345\AppData\Local\Temp\nova-piper-sample.wav`（「你好呀，我是小诺，现在用的是离线语音，不用联网也能说话。」，5.35 s，22.05 kHz，RMS 5643）。

### 2.2 Rust 侧实现（可编译、可单测、离线端到端已验证）

1. **`src-tauri/src/piper_tts.rs`（新增，核心模块）**
   - 运行时/音色清单以常量固化：URL、字节数、sha256（`RUNTIME`、`VOICES`、`RUNTIME_VERSION`、`DEFAULT_VOICE = "zh_CN-huayan-medium"`）。
   - `install(root, voice_id, progress)`：下载运行时 zip + 音色 `.onnx`/`.onnx.json`，逐个校验**字节数 + sha256**，不匹配就删掉并报错；zip 解包用 `enclosed_name()` 防路径穿越，剥掉压缩包里的顶层 `piper/` 目录。
   - `synthesize(root, text, voice_id, rate)`：把文本经 `speech_text::prepare_for_speech` 清洗后写进 `piper.exe` 的 stdin，用 `--output_file` 输出 WAV 到 `tempfile::tempdir()`，读回字节返回（RIFF 头校验、25 MB 上限、Windows 下 `CREATE_NO_WINDOW` 避免黑框）。失败时把 piper stderr 最后一行带回错误信息。
   - `status(root, selected_voice)`：供 UI 展示「运行时是否就绪 / 每个音色是否已装 / 还差多少字节」。
   - 目录布局：`<app_data>/piper/runtime/{piper.exe,*.dll,espeak-ng-data/}`、`<app_data>/piper/voices/<voice>.onnx(.json)`。
   - `SUPPORTED = cfg!(windows && x86_64)`：目前只有 Windows x64 有预编译运行时，其它平台 `install/synthesize` 直接返回可读的配置错误。
   - rate → `--length_scale = 1/(1+rate/100)`，clamp 到 `[0.5, 2.0]`（`length_scale()`）。
   - 单测 9 个（其中 8 个常驻 + 1 个 `#[ignore]` 的下载+离线合成端到端测试：清单自检、状态、参数校验、length_scale 映射、坏压缩包等）。

2. **`src-tauri/src/speech_text.rs`（新增）**
   - 把原 `edge_tts.rs` 里的 Markdown→可朗读文本清洗（`prepare_for_speech` 及其 4 个私有辅助函数、2 个单测）整体搬出来，供两个 provider 共用，避免两边文本不一致。

3. **`src-tauri/src/commands/mod.rs`**
   - 新增 `SpeechProvider { Piper, Edge }`（serde `lowercase`）。
   - `save_settings`：按 provider 分别校验（piper 走 `piper_tts::validate_voice` + `validate_rate`；edge 保持原 `edge_tts::validate_options`），并回写规范化后的 provider 字符串。
   - `synthesize_speech`：签名变为 `(app, database, text, options?, provider?)`，provider 缺省时读 `settings.tts_provider`；piper 返回 `audio/wav`（base64），edge 仍返回 `audio/mpeg`。
   - 新增 `get_piper_status`、`install_piper_voice`（`spawn_blocking` 里下载，进度通过 `app.emit("nova:piper-progress", {phase, receivedBytes, totalBytes})` 上报，每 512 KB 一次；`phase` 为 `runtime`|`voice`）。
   - 新增 `app_data_root(app)` 辅助函数；若干「暂不支持自定义语音合成」文案改为提到 Piper/Edge。

4. **`src-tauri/src/infrastructure/database.rs`**
   - `AppSettings` 增加 `tts_provider: String`；`get_settings`/`save_settings` 的 SQL 同步。
   - 迁移 8：`ALTER TABLE app_settings ADD COLUMN tts_provider TEXT NOT NULL DEFAULT 'piper'`，随后按平台把默认值写成 `DEFAULT_TTS_PROVIDER`/`DEFAULT_TTS_VOICE`（Windows x64 → `piper` + `zh_CN-huayan-medium`，其它平台 → `edge` + `zh-CN-XiaoxiaoNeural`，常量由 `piper_tts::SUPPORTED` 推导）。
   - 2 个既有设置单测更新为新字段。

5. **`src-tauri/src/lib.rs`**：注册 `mod piper_tts; mod speech_text;` 与两个新命令。

6. **`src-tauri/Cargo.toml` / `Cargo.lock`**：新增 `zip = { version = "=2.4.2", default-features = false, features = ["deflate"] }`（纯 Rust flate2/miniz_oxide，不需要 C 工具链）；`tempfile` 从 dev-dependencies 提到 dependencies（合成时放临时 WAV）。

### 2.3 验证结果（当前 working tree 实测）

| 检查 | 命令 | 结果 |
| --- | --- | --- |
| Rust 编译 | `npm run check:rust` | ✅ 通过，**0 warning** |
| 前端类型 | `npm run typecheck` | ✅ 通过（前端尚未改动，自然通过） |
| Rust 单测 | `npm run test:rust` | ⚠️ **62 passed / 1 failed / 2 ignored**，失败项为新增测试自身的 bug（K-1） |
| 离线端到端 | `NOVA_PIPER_TEST_ROOT=<C: 上的路径> node scripts/cargo.mjs test --manifest-path src-tauri/Cargo.toml installs_and_synthesizes_offline -- --ignored --nocapture` | ✅ **通过**（真实下载 22 MB 运行时 + 63 MB 模型 → 校验 → 解包 → 离线合成出 RIFF WAV） |
| 应用启动 | `npm run tauri dev` | ✅ 改完后端后 dev watcher 自动重编译并成功启动 `target\debug\nova.exe`（已在本轮结束前停掉进程） |
| 真实数据库 | 读取 `%APPDATA%\app.nova.companion\nova.db` | ✅ 已迁移到 `schema_migrations = 8`，`app_settings = ('piper', 'zh_CN-huayan-medium', -5)` |

> 说明：端到端测试在 **D: 盘**上会失败（K-2，文件系统层面的目录 rename 限制），在 **C: 盘**（即真实 app data 目录所在盘）通过。上面表格中的 ✅ 是 C: 盘结果。

### 2.4 明确「还没有做」的部分

- 前端（`src/lib/commands.ts`、`src/App.tsx`）**完全没有动**：没有 provider 选择、没有语音包下载按钮/进度、没有 piper 音色列表、没有把 provider 传给后端、`AppSettings` 里也没有 `ttsProvider`。
- 文档未更新（`docs/tts-options-research.md`、`docs/tech-architecture.md`、`docs/tasks.md`）。
- 未提交、未 push。

---

## 3. 修改过的主要文件及修改目的

| 文件 | 状态 | 修改目的 |
| --- | --- | --- |
| `src-tauri/src/piper_tts.rs` | 新增（`??`） | Piper 离线 TTS：清单常量、下载+校验+解包、sidecar 合成、状态查询 |
| `src-tauri/src/speech_text.rs` | 新增（`??`） | 抽出共用的 Markdown→朗读文本清洗 |
| `src-tauri/src/edge_tts.rs` | 修改（+4/−178） | 只保留 Edge WebSocket 协议实现，清洗逻辑改为 `use crate::speech_text::prepare_for_speech` |
| `src-tauri/src/commands/mod.rs` | 修改（+139/−31） | provider 枚举、按 provider 校验设置、`synthesize_speech` 分发、`get_piper_status`、`install_piper_voice`、进度事件常量 |
| `src-tauri/src/infrastructure/database.rs` | 修改（+63/−11） | `tts_provider` 列 + 迁移 8 + 平台相关默认值 |
| `src-tauri/src/lib.rs` | 修改 | 注册新模块与两个新命令 |
| `src-tauri/Cargo.toml` / `Cargo.lock` | 修改 | 加 `zip`（仅 deflate）、`tempfile` 转正常依赖 |
| `TASK.md` | 新增（`??`） | 本交接文档 |

`git status --short` 应显示：`M src-tauri/Cargo.lock`、`M src-tauri/Cargo.toml`、`M src-tauri/src/commands/mod.rs`、`M src-tauri/src/edge_tts.rs`、`M src-tauri/src/infrastructure/database.rs`、`M src-tauri/src/lib.rs`、`?? src-tauri/src/piper_tts.rs`、`?? src-tauri/src/speech_text.rs`。（前端 `src/**` 无改动。）

---

## 4. 当前实现思路与重要技术决策

1. **sidecar 而不是链接库**：Piper 作为独立进程调用（不编译任何 C/C++ 依赖），符合项目「不在用户机器上编译原生依赖」的既有约束。代价是每次合成都重新加载模型（约 0.4 s）。
2. **不随安装包塞模型**：运行时与音色都在设置页按需下载到 app data 目录（`bundle.active = false`，当前也没有把 piper 打包成 Tauri resource）。
3. **完整性优先**：所有远端资源都在代码里 pin 了 URL + 字节数 + sha256；长度不符或哈希不符直接丢弃并报错（对应调研文档「下载前校验 SHA-256」）。
4. **共用文本清洗**：两个 provider 必须朗读同一份文本，所以把清洗提到 `speech_text`，而不是各写一份。
5. **默认 provider = Piper（Windows x64），其它平台默认仍为 Edge**：Piper 目前只有 Windows x64 预编译运行时，非 Windows 平台默认值和 UI 都必须回退到 Edge（常量 `DEFAULT_TTS_PROVIDER`/`DEFAULT_TTS_VOICE`）。
6. **保留 Edge TTS**：作为在线备选与回归对照，`speech` 能力仍然不支持自定义 OpenAI 兼容 TTS API。
7. **rate 语义复用**：piper 没有 pitch/volume，只有 `length_scale`；用 `1/(1+rate/100)` 映射既有语速设置（-50..+100），保持设置项语义不炸。

---

## 5. 已知问题、错误和风险

### K-1（必须修，测试自身 bug，极小）
`piper_tts::tests::reports_a_ready_runtime_and_installed_voice` 失败：
```
voice file: Os { code: 3, kind: NotFound, ... }
```
原因：测试直接往 `<root>/piper/voices/` 写文件，但没有先 `create_dir_all`。
修法：写文件前加 `std::fs::create_dir_all(root.path().join(super::VOICE_DIRECTORY))`（仅测试代码）。
影响：`npm run test:rust` 目前是红的。这是**目前唯一失败的测试**。

### K-2（真实健壮性问题，Windows 卷差异）
`install_runtime` 用 `fs::rename(staging → runtime)` 启用运行时，在 **D: 盘**上必然失败：
```
安装运行时失败：Internal("无法启用语音运行时：拒绝访问。 (os error 5)")
```
已定位：不是代码 bug 的正确性问题，而是本机 D: 卷**不允许 rename 含子目录的目录**（`mkdir a/b; mv a c` 同样 `Permission denied`；`cmd move` 也一样；C: 盘正常）。这也让 `NOVA_PIPER_TEST_ROOT` 落到 D: 盘时端到端测试失败。
真实 app data 目录在 C:（`%APPDATA%\app.nova.companion`），因此当前实现**在真实路径上可用**（已实测通过）。
建议加固（二选一）：
- 直接解包到最终 `runtime` 目录（先删旧目录，解包后校验 `piper.exe` 存在）——最省事，但失去「先解包再原子替换」的语义；
- 保留 staging，但 `rename` 失败时回退为「递归复制 + 删 staging」，并把失败原因写进日志。

### K-3（当前 working tree 的阻断级状态，接手第一件事）
前端没改，但后端已经要求新字段/新 provider，导致：
- 设置页保存会失败：`AppSettings` 的 `tts_provider` 是必填字段（无 `#[serde(default)]`），前端不传 `ttsProvider` → `save_settings` 报 `missing field 'ttsProvider'`。
- 聊天页朗读会失败：数据库已经是 `provider='piper'`，而语音包未安装 → 报「离线语音运行时尚未就绪，请在语音设置中下载」。
- 本轮结束后端进程已停；下次 `npm run tauri dev` 就是上面这个半成品状态。
建议：先做「下一步 P0」，或在完成前端前临时给 `tts_provider` 加 `#[serde(default = "…")]` 兜底。

### K-4 许可与来源（需在文档里说清，属于产品决策）
- Piper 运行时 `2023.11.14-2` = MIT（✅ 可分发）。
- `rhasspy/piper-voices` 仓库标 MIT，但**具体音色数据集许可不同**：`huayan` 的数据集 `PlayVoice/HuaYan_TTS` 在 model card 里写的是 **License: Unknown**（⚠️ 需产品确认）；许可最干净的 `chaowen`（数据集 CC0）**不兼容本项目 pin 的运行时**（K-5）。
- `piper1-gpl` ≥1.3 是 **GPL-3.0**，且没有独立可执行文件，本项目不使用。

### K-5 音色兼容性
`zh_CN-chaowen-medium`、`zh_CN-xiao_ya-medium` 在 pin 的运行时下直接报 `"ai" is not a single codepoint (ids=30,)`（需要 Python piper 1.4+/g2pW）。已从清单排除；如果以后要用 CC0 音色，需要先解决运行时版本问题（自建二进制或引入 Python 运行时，均与本项目「不编译原生依赖」约束冲突）。

### K-6 功能能力差异
Piper 不支持音调/音量（只有语速）。前端在 piper 模式下应隐藏「声调/音量」滑块，否则用户会以为设置生效。

### K-7 时延（与 TASK-016 相关，未量测）
每次合成都是新进程 + 重新加载模型（约 0.4 s 固定开销 + RTF≈0.06 的推理）。`docs/tasks.md` 的 TASK-016 只针对 Edge 量测过「首帧延时」，**换成 Piper 后需要重新按第 6 节口径量测**（前端已经按句切分流水线播放，单句文本短，进程启动开销占比可能明显）。
可选优化：常驻一个 piper 进程（stdin 逐行输入 + `--output_raw`），但要处理进程生命周期与并发，风险中等。

### K-8 其它小风险
- 进度事件每 512 KB 一条（63 MB ≈ 120 条），且 `phase` 是「单个文件」维度，前端若要展示总进度需要用 `get_piper_status` 的 `runtimeBytes`/`downloadBytes` 自行折算。
- 下载使用 `reqwest::blocking`（connect 30 s / 总 30 min）；本机需要代理环境变量才能访问 GitHub/HuggingFace（见第 7 节）。成功后没有断点续传，失败需要重来（`.part` 文件会被覆盖）。
- 校验只对「模型文件大小」做安装态判断（`voice_installed` 比字节数），运行时只判断 `piper.exe` 是否存在；不做每次启动的全量哈希校验（有意为之，避免每次启动读 22 MB）。
- `synthesize` 的文本上限沿用 `required_text(..., 8_000)`；8000 字符中文最坏约 24 KB，小于管道缓冲区，stdin 一次性写入不会死锁。

---

## 6. 尚未完成的工作（清单）

1. 前端接线（`src/lib/commands.ts`、`src/App.tsx`）：provider 选择、Piper 音色列表、下载/进度 UI、`synthesizeSpeech` 传 provider、`AppSettings.ttsProvider`、piper 模式下隐藏声调/音量。
2. `AppSettings.tts_provider` 的 serde 兼容兜底（`#[serde(default = ...)]`），避免前端/旧数据缺字段时整体失败。
3. 修 K-1（失败的单元测试）。
4. 修/加固 K-2（D: 卷 rename 失败）。
5. 文档更新：
   - `docs/tts-options-research.md`：把「首选 sherpa-onnx、当前固定 Edge TTS」改为「已落地 Piper 离线 + Edge 在线」，记录 K-4 的许可核查结论与 K-5 的兼容性实测。
   - `docs/tech-architecture.md`：`MODULE-002`/`app_settings` 列表/远端依赖表/ADR-007 都写着「TTS 固定 Edge TTS」，需要更新为 piper 默认 + edge 备选，并补 `tts_provider` 列。
   - `docs/tasks.md`：TASK-014/TASK-016 的描述里提到「Edge TTS 的音色、语速、音调和音量」；建议新增一条 Piper 任务行（并注明尚需实机验收）。
6. 应用内人工验收（用 UI 走通：下载语音包 → 试听 → 聊天朗读 → 自动播放），以及在 C: 盘之外环境确认 K-2。
7. `git commit` + `push`。

---

## 7. 下一步应该做什么（按优先级）

**P0（先让工程回到「可运行、可测」状态）**
1. 修 K-1：测试里先建 `piper/voices` 目录，`npm run test:rust` 必须全绿。
2. 前端最小闭环（`src/lib/commands.ts` + `src/App.tsx`）：
   - `AppSettings` 加 `ttsProvider: "piper" | "edge"`（`defaultSettings` 同步为 `"piper"` + `ttsVoice: "zh_CN-huayan-medium"`）。
   - 新增 TS 包装：`getPiperStatus()`、`installPiperVoice(voiceId)`；`synthesizeSpeech(text, options?, provider?)`。
   - 后端事件名 `nova:piper-progress`，payload `{ phase: "runtime" | "voice", receivedBytes, totalBytes }`，用 `listen` 订阅（参考 `App.tsx` 里 `listen("nova:conversation-cleared", ...)` 的写法）。
   - TTS 卡片：provider 单选（离线 Piper / 在线 Edge）、piper 模式下按 `PiperStatus.voices[]` 列音色并显示已装/未装、显示 `pendingBytes` 与下载按钮、`supported === false` 时禁用并提示平台不支持；piper 模式隐藏「声调/音量」。
   - `playSpeech`/试听把当前 provider 传下去（否则后端只会读已保存的设置）。
3. 应用内跑一次：设置页下载 `zh_CN-huayan-medium` → 试听 → 聊天页朗读 + 自动播放，确认 `audio/wav` 能在 `new Audio(...)` 里播放。
4. `git add -A && git commit`（换电脑前必须完成）。

**P1**
5. 加固 K-2（rename 回退或直接解包到最终目录），并让 `NOVA_PIPER_TEST_ROOT` 指向 C: 盘时端到端测试稳定通过。
6. 按第 5 节 K-7 用固定文本量测 piper 的首帧/整句延时，更新 `docs/tasks.md` TASK-016 的数据；决定是否需要常驻进程优化。
7. 文档更新（第 6 节第 5 条）。

**P2**
8. 是否把 Piper 运行时作为 Tauri resource 随包分发（省掉 22 MB 下载，但会引入 MIT 二进制再分发与体积问题）；是否允许用户自选本地 `.onnx` 模型（当前只支持 pin 住的两个音色）。
9. 断点续传/失败重试、下载镜像（HF 在部分网络下不可达）。

---

## 8. 继续工作时需要注意的上下文

- **跑应用的命令**：`npm run tauri dev`（会自动执行 `scripts/prepare-native.mjs` 复制 Windows SDK 的 `winsqlite3.lib`；`native/sqlite/windows-x64` 已就绪）。`npm run dev` 只起 Vite 页面，不是完整应用。
- **质量检查**：`npm run check`（= `typecheck` + `test:rust`）；`npm run check:rust` 只编译。
- **Rust 版本**：`cargo/rustc 1.98.1`，与 `Cargo.toml` 的 `rust-version = "1.98.1"` 一致；`edition = 2024`。
- **网络**：本机访问 GitHub/HuggingFace 需要代理（实测环境变量 `HTTPS_PROXY=http://127.0.0.1:7897`），`reqwest` 默认读取该变量；换机后若无代理，下载会失败。
- **本机 D: 卷怪癖（K-2）**：任何「重命名含子目录的目录」都会 `Permission denied`，写测试/脚本时不要把需要 rename 的临时目录放在 D: 盘。
- **可复用的验证产物**（本机，不入库）：
  - `C:\Users\ZCG04000345\AppData\Local\Temp\piper-verify-c\`：已装好的 `piper/runtime` + `piper/voices/zh_CN-huayan-medium.onnx`，可直接 `NOVA_PIPER_TEST_ROOT` 复用或拷进 `%APPDATA%\app.nova.companion\piper\` 免去重新下载 85 MB。
  - `C:\Users\ZCG04000345\AppData\Local\Temp\nova-piper-sample.wav`：离线合成样本，供人耳确认音质。
  - `%TEMP%\piper-test\`：最早的手工实验目录（含 piper 原始 zip、多个中文模型，其中 `chaowen`/`xiao_ya` 为不兼容样本）。
- **数据库**：真实库 `%APPDATA%\app.nova.companion\nova.db` **已经迁移到版本 8** 且 `tts_provider='piper'`。如果前端暂时接不上，可以在设置页把 provider 改回 `edge`（前提是先把 K-3 的必填字段问题处理掉）。
- **后台进程**：本轮结束前已停止 `nova.exe`、`scripts/tauri.mjs dev` 与 Vite（1420 端口无监听），不会占构建锁。
- **未验证/半成品标注**：前端部分（第 6 节 1–2）**完全未实现、未验证**；UI 交互、进度条、音频播放、时延数据均无实测；文档与真实实现之间存在不一致（第 6 节第 5 条）。

---

## 9. 一句话结论

后端 Piper 离线中文语音已**跑通并端到端验证**（含真实下载、哈希校验、解包、离线合成出 WAV）；**前端接线、1 个失败单测、D: 盘 rename 加固、文档更新、提交**都还没做，接手请从第 7 节 P0 开始。
