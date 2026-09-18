# 免费 TTS 方案调研（Windows 桌面端）

调研日期：2026-09-18。

## 结论

首选 **sherpa-onnx + 可单独下载的中文 ONNX 模型**。它可完全离线运行，不需要 API Key，也不需要在用户电脑上编译 C/C++ 依赖；官方同时提供 Windows 的预编译发布物和 TTS 模型下载。应用首次启用语音时下载对应的 x64 运行时与一个用户选择的语音模型，后续在本机合成并播放。

这最符合 Nova 的本地优先方向和“不要在本机编译依赖”的约束。运行时源码是 Apache-2.0；**模型权利必须按最终选用的具体模型再次核验**，不要将“运行时开源”误当成全部模型均可自由再分发。

## 当前实现决定

Nova 当前固定使用 **Rust 直接实现的 Edge TTS WebSocket 客户端**：不依赖 Python、`edge-tts` CLI 或外部 sidecar。它仅在用户点击朗读或开启自动朗读时将待朗读文本发送至 Edge 在线服务，并将 MP3 字节返回给前端内存播放。

设置页只开放 Edge TTS 的可控参数：音色、语速、音调和音量。当前版本暂不支持自定义 OpenAI 兼容语音合成接口；对话和语音识别仍可分别配置 OpenAI 兼容 API。

## 候选对比

| 方案 | 成本与联网 | 中文与声音选择 | Windows 集成 | 许可证/风险 | 建议 |
| --- | --- | --- | --- | --- | --- |
| sherpa-onnx（VITS / MeloTTS 模型） | 免费、离线 | 有中英模型，及 5/174/187/804 说话人模型；模型约 115–163 MB | 官方有预编译发布物；可由 Tauri 后端调用独立 exe | runtime 为 Apache-2.0；逐个审查模型许可证 | **首选** |
| Kokoro 82M + 本地 OpenAI 兼容服务 | 免费、离线 | 中文优化检查点有 8 个普通话音色 | 需带 Docker/Python+ONNX Runtime 服务；首次模型下载约 330 MB | Kokoro 仓库为 Apache-2.0；部署体积与服务管理较重 | 质量优先时的备选 |
| edge-tts | 无 API Key，但必须联网 | Microsoft Neural 音色丰富，中文效果通常好 | Rust 直接实现协议，不需要 Python/sidecar；服务端变更会导致失效 | 项目主体 LGPLv3；还依赖未承诺的 Edge 在线服务 | 当前版本采用；设置页开放音色、语速、音调和音量 |

## 证据与实现线索

### sherpa-onnx

- 官方 TTS 文档列出 `vits-melo-tts-zh_en`（中英、163 MB），`sherpa-onnx-vits-zh-ll`（中文、5 人声）和多说话人中文模型；其中 Aishell3 为 174 人声。见 [官方模型目录](https://k2-fsa.github.io/sherpa/onnx/tts/pretrained_models/vits.html)。
- 官方 FAQ 说明 `pip install sherpa-onnx` 不需要 C++ 编译器，并支持 Windows x64；同时提供 `sherpa-onnx-offline-tts` 命令。Nova 不应采用 pip 安装，而应下载并校验其 GitHub Release 中的预编译 Windows x64 可执行文件/动态库。见 [官方 FAQ](https://k2-fsa.github.io/sherpa/onnx/tts/faq.html) 和 [发布页](https://github.com/k2-fsa/sherpa-onnx/releases)。
- 项目自己的 Windows MFC 示例明确给出“不想自行编译时下载预编译 exe”的路径，并包含非流式 TTS 示例。见 [MFC 示例说明](https://github.com/k2-fsa/sherpa-onnx/blob/master/mfc-examples/README.md)。
- 模型文件来自不同训练项目；例如中英 MeloTTS 模型的发布包中单独带有 `LICENSE` 文件。因此下载器需要保存模型元数据（来源、版本、SHA-256、许可证、是否允许再分发），并在打包前审查。

### Kokoro

- [hexgrad/kokoro](https://github.com/hexgrad/kokoro) 标注 Apache-2.0。社区的 OpenAI 兼容实现列出了专用普通话检查点和 8 个中文音色（4 女、4 男）。见 [Kokoro OpenAI API 的语音目录](https://github.com/seancheung/kokoro-openai-tts-api)。
- 另一个可参考的开源服务说明，其首次启动会下载约 330 MB 主模型，单个音色 embedding 小于 1 MB；可使用 CPU 模式并暴露 `POST /v1/audio/speech`。见 [OpenTTSGroup/kokoro-open-tts](https://github.com/OpenTTSGroup/kokoro-open-tts)。
- 它更适合作为用户自行部署的 OpenAI 兼容 TTS 服务，或后续“高质量本地语音包”；不适合作为当前桌面客户端的首个内置方案。

### edge-tts

- [rany2/edge-tts](https://github.com/rany2/edge-tts) 可在不使用 API Key 的情况下调用 Microsoft Edge 的在线语音服务，并支持音色、语速、音量和音调。
- 但其 [许可证](https://github.com/rany2/edge-tts/blob/master/LICENSE) 说明除一个文件外其余代码为 LGPLv3；该实现也会随微软端点变更而调整。因此“免费”不等于适合内置、长期稳定的产品能力。

## 后续可选方向

1. 如需完全离线，再将 TTS 抽象为 `SpeechProvider`，增加 `LocalSherpaProvider`；当前 Edge TTS 实现保持不变。
2. 默认不随安装包塞入模型；设置页提供“下载免费离线语音包”，显示下载体积、许可证与删除按钮。
3. 首个预置包采用一个已复核许可的中文模型；音色选择只暴露该模型实际支持的 speaker/voice。
4. 后端把文本切句、调用预编译 `sherpa-onnx-offline-tts`、输出 WAV 到应用缓存；前端直接播放并在完成后清理缓存。
5. 下载前校验 SHA-256，运行时与模型按 Windows x64 / ARM64 分开管理；不在用户机器上编译任何原生依赖。

## 尚需决定

- 是否允许用户手动选择任意本地 ONNX 模型，还是只提供我们验证过的语音包。
- 首版优先“完全离线、可控许可”，还是优先“更自然的云端神经音色”。
- 是否愿意接受约 120–180 MB 的首次语音包下载；若接受，sherpa-onnx 的本地方案最平衡。
