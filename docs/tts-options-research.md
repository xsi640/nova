# TTS 方案调研与选型（Windows 桌面端）

调研日期：2026-09-18；最近更新：2026-09-29。

## 当前实现决定

Nova 支持两种语音合成方式，由 `app_settings.tts_provider` 切换，设置页**先选方式，再配置该方式的参数**：

- **`edge`（默认，免费、无需 Key）**：Rust 直接实现的 Edge TTS WebSocket 客户端，朗读时把文本发送到 Edge 在线服务并返回 MP3。设置项：音色、语速、音调、音量。
- **`volcengine`（火山引擎豆包语音合成 2.0，需 API Key）**：走原生 **v3 双向流式 WebSocket**（`wss://openspeech.bytedance.com/api/v3/tts/bidirection`，资源 ID `seed-tts-2.0`），模型默认 `seed-tts-2.0-standard`（可选 `seed-tts-2.0-expressive`），默认音色「小何 2.0」（`zh_female_xiaohe_uranus_bigtts`），输出 MP3。鉴权用新控制台的单个 **API Key**（请求头 `X-Api-Key`）。设置项：API Key（存系统凭据存储）、模型版本、音色、语速、音量。

两种方式共用同一份「Markdown → 可朗读文本」清洗（`speech_text::prepare_for_speech`），保证朗读内容一致。当前版本不支持用户自定义任意 OpenAI 兼容 TTS 服务（只内置上述两种方式）。

## 选型结论（按目标音质）

- **想接近豆包/商用神经 TTS 的音质**：必须走云端同量级服务。已内置火山引擎豆包语音合成 2.0（Seed-TTS，`*_uranus_bigtts` 音色），这是最直接的路径，也和小智 AI 等同类产品的「好听档」一致（见 `xiaozhi-tts-research.md`）。
- **想免费、零配置**：用默认的 Edge TTS。它比本地小模型自然得多，但仍偏「播报感」，达不到豆包水平。
- **想完全离线**：本机需要更大的现代模型（CosyVoice2 / GPT-SoVITS / IndexTTS 等），代价是 Python/PyTorch 或 ONNX + 数 GB 模型，与本项目「轻量、不在本机编译原生依赖」的约束冲突较大，暂不内置。

## 候选对比

| 方案 | 成本与联网 | 中文与声音选择 | Windows 集成 | 许可证/风险 | 结论 |
| --- | --- | --- | --- | --- | --- |
| Edge TTS | 免费、需联网 | Microsoft Neural 音色较丰富，中文效果好于本地小模型 | Rust 直接实现协议，无需 Python/sidecar | 依赖未承诺的 Edge 在线服务，服务端变更会导致失效 | **默认采用** |
| 火山引擎豆包语音合成 2.0（Seed-TTS） | 付费（有额度），需联网 | 大模型 2.0 音色（`*_uranus_bigtts`，如小何 2.0），接近商用水准 | v3 双向流式 WebSocket，单个 API Key（`X-Api-Key`） | 商用服务，需火山账号与 API Key；模型/音色由火山侧决定 | **可选采用**（音质目标） |
| Piper（`rhasspy/piper` 预编译运行时） | 免费、离线 | 中文音色为 2023 年 VITS 小模型，机械感明显 | sidecar exe，无需编译原生依赖 | 运行时 MIT；`huayan` 数据集许可 Unknown | 已实现并评估，**因音质不达标已移除** |
| sherpa-onnx（VITS / MeloTTS） | 免费、离线 | 中英模型多，但仍是中小模型 | 有预编译发布物，可做 sidecar | runtime Apache-2.0；模型许可需逐个核 | 有本地离线需求时的备选 |
| CosyVoice2 / GPT-SoVITS / IndexTTS（本地） | 免费、离线 | 质量接近商用 | 需 Python/PyTorch 或 ONNX + 数 GB 模型，最好有 GPU | 各自许可不同，需逐个核 | 质量优先且接受重依赖时的后续方向 |
| Kokoro 82M + 本地服务 | 免费、离线 | 中文一般 | 需 Docker/Python+ONNX Runtime 服务 | Apache-2.0 | 不优先 |

## 关键事实与来源

- **火山引擎豆包语音合成 2.0**：原生 v3 双向流式 WebSocket `wss://openspeech.bytedance.com/api/v3/tts/bidirection`；资源 ID `seed-tts-2.0`（声音复刻为 `seed-icl-2.0`）；`req_params.model` 取 `seed-tts-2.0-standard`（标准）/ `seed-tts-2.0-expressive`（表现力）；音色为 `*_uranus_bigtts`（小何 2.0 = `zh_female_xiaohe_uranus_bigtts`）；鉴权头 `X-Api-App-Id` / `X-Api-App-Key` / `X-Api-Access-Key` / `X-Api-Resource-Id` / `X-Api-Request-Id` / `X-Api-Connect-Id`；二进制帧 + `BidirectionalTTS` 事件序列（StartConnection → StartSession → TaskRequest → FinishSession → FinishConnection）。官方文档 <https://www.volcengine.com/docs/6561/1329505>。参考实现：[`Hypnus-Yuan/doubao-speech`](https://github.com/Hypnus-Yuan/doubao-speech) 的 `_ws_client.py`、xiaozhi 社区服务端 `huoshan_double_stream.py`、[MOSShell](https://github.com/GhostInShells/MOSShell)。
- **鉴权**：新控制台只需单个 **API Key**，作为 `X-Api-Key` 请求头发送（旧控制台的 App ID + Access Token 也可，但本项目只实现 API Key）；另需 `X-Api-Resource-Id` / `X-Api-Request-Id` / `X-Api-Connect-Id`。实测：对端点发假 Key 返回 `401 {"error":"Invalid X-Api-Key"}`，证明头名正确。
- **Edge TTS**：[rany2/edge-tts](https://github.com/rany2/edge-tts) 可免 Key 调用 Microsoft Edge 在线语音、支持音色/语速/音量/音调；其[许可证](https://github.com/rany2/edge-tts/blob/master/LICENSE) 除一个文件外为 LGPLv3。Nova 未使用该 Python 项目，而是用 Rust 直接实现协议。
- **Piper（历史）**：运行时 `rhasspy/piper` release `2023.11.14-2`（MIT），中文音色 `zh_CN-huayan-medium` / `zh_CN-huayan-x_low`；`huayan` 数据集许可标注 Unknown；`chaowen` / `xiao_ya` 在 pin 的运行时下报 `"ai" is not a single codepoint`，不兼容。实测模型加载约 0.4 s、RTF 0.06–0.13，但音质不满足「和豆包差不多」的目标，故整体移除。
- **sherpa-onnx**：官方提供 Windows x64 预编译发布物与 `sherpa-onnx-offline-tts`；模型来自不同训练项目，需保存并核验每个模型的来源/许可/SHA-256。[官方模型目录](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/vits.html)、[发布页](https://github.com/k2-fsa/sherpa-onnx/releases)。

## 后续可选方向

1. 火山 v3 已支持双向流式：把后端从「整段返回」升级为「边合成边回传音频」，进一步降低 TTS 首帧延时（见 `tasks.md` TASK-016）。
2. 增加「自定义 OpenAI 兼容 TTS」入口，让用户接入任意 `/audio/speech` 服务（当前只内置 Edge 与火山引擎）。
3. 完全离线需求若成立，再评估 CosyVoice2/GPT-SoVITS 作为可选本地引擎（重依赖、体积大，需单独立项）。

## 尚需决定

- 火山引擎方式默认音色（当前 `zh_female_xiaohe_uranus_bigtts`，小何 2.0）。
- 是否同时支持火山「声音复刻 2.0」资源（`seed-icl-2.0`）与克隆音色。
- 是否开放「自定义 OpenAI 兼容 TTS」配置。
