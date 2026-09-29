# 小智 AI（XiaoZhi）用的什么 TTS 引擎？

调研日期：2026-09-29。结论优先，来源逐条给出。

## 一句话结论

小智 AI **没有单一固定的 TTS 引擎**：设备固件不做合成，TTS 在服务端按配置选择。它的「好听档」实际就是**火山引擎 / 字节跳动豆包大模型语音合成（`*_moon_bigtts`）**；作者自带的参考服务里还并列了**阿里云 DashScope CosyVoice**，而最流行的社区服务端默认用的是**免费 Edge TTS**。

## 1. 固件层不做 TTS

`78/xiaozhi-esp32`（官方固件，默认接入 `xiaozhi.me` 官方服务器）在 README 里只描述「Opus 音频流，支持传统 ASR + LLM + TTS 方案，也支持 Realtime 端到端语音模型」，没有指定任何 TTS 引擎。

- 来源：[78/xiaozhi-esp32 README_zh.md#L27](https://github.com/78/xiaozhi-esp32/blob/main/README_zh.md)（"采用 Opus 音频流，既支持传统的流式 ASR + LLM + TTS 方案……"）
- 来源：[78/xiaozhi-esp32 README_zh.md#L110](https://github.com/78/xiaozhi-esp32/blob/main/README_zh.md)（"固件默认接入 xiaozhi.me 官方服务器"）

## 2. 作者原始后端：火山字节 + 阿里 DashScope，默认豆包 bigtts

作者自己的后端仓库 `78/xiaozhi`（已停止维护）里有 `tts-server/`，直接给出两种客户端：

```js
// tts-server/app.js
const ttsClients = {
  volcengine: BytedanceTtsClient,
  dashscope: DashscopeTtsClient,
};
```

- **火山引擎 / 字节跳动**（`tts-server/bytedance_tts.js`）：
  - 协议：`wss://openspeech.bytedance.com/api/v3/tts/bidirection`（大模型语音合成**双向流式** API），`X-Api-Resource-Id: volc.service_type.10029`；
  - 默认 speaker：`zh_female_shuangkuaisisi_moon_bigtts`（爽快思思）；
  - 出处：[bytedance_tts.js](https://github.com/78/xiaozhi/blob/main/tts-server/bytedance_tts.js)
- **阿里云 DashScope**（`tts-server/dashscope_tts.js`）：
  - `this.model = 'cosyvoice-v1'`，默认 voice `longjielidou`；
  - 出处：[dashscope_tts.js](https://github.com/78/xiaozhi/blob/main/tts-server/dashscope_tts.js)
- **音色目录**（`tts-server/tts-list.js`）：共 46 个音色，按 `voice_source` 统计为 **volcengine 26 个 + dashscope 20 个**；火山音色 ID 多为 `*_moon_bigtts`（如 `zh_female_shuangkuaisisi_moon_bigtts`、`zh_male_wennuanahu_moon_bigtts`）。
  - 出处：[tts-list.js](https://github.com/78/xiaozhi/blob/main/tts-server/tts-list.js)

> 注意：`78/xiaozhi` 是作者早期/参考实现，并不等于 `xiaozhi.me` 线上服务的内部实现；官方云服务闭源，未公开其 TTS 供应商。

## 3. 社区主流服务端：默认 Edge，豆包是「好听档」

最流行的开源服务端 `xinnan-tech/xiaozhi-esp32-server`（star 约 1.07 万）在 `main/xiaozhi-server/config.yaml` 里：

- 默认选择：`selected_module.TTS: EdgeTTS`，其默认音色 `zh-CN-XiaoxiaoNeural`；
- 配置注释：「当前支持的 type 为 edge、doubao」；
- 同文件并列的 TTS 配置块（`type:`）覆盖：
  `edge`、`doubao`、`huoshan_double_stream`、`siliconflow`（CosyVoice 托管）、`cozecn`、`openai`（OpenAI 兼容网关）、`fishspeech`、`gpt_sovits_v2`/`gpt_sovits_v3`、`minimax_httpstream`、`aliyun` / `aliyun_stream`、`tencent`、`xunfei_stream`、`index_stream`、`paddle_speech`；
- 豆包块使用 `https://openspeech.bytedance.com/api/v1/tts`，默认 `voice: BV001_streaming`，注释说明「火山引擎语音一定要购买花钱，起步价 30 元……免费只有 2 个并发，会经常报 tts 错误」；大模型音色（如 `zh_female_wanwanxiaohe_moon_bigtts` 湾湾小何）走 `wss://openspeech.bytedance.com/api/v3/tts/bidirection`（`huoshan_double_stream`，`resource_id: volc.service_type.10029`）。

- 代码目录证据：[`main/xiaozhi-server/core/providers/tts/`](https://github.com/xinnan-tech/xiaozhi-esp32-server/tree/main/main/xiaozhi-server/core/providers/tts)（含 `doubao.py`、`huoshan_double_stream.py`、`edge.py`、`aliyun.py`、`siliconflow.py`、`minimax_httpstream.py`、`fishspeech.py`、`gpt_sovits_v2.py`、`gpt_sovits_v3.py`、`openai.py`、`cozecn.py`、`tencent.py`、`xunfei_stream.py`、`index_stream.py`、`paddle_speech.py` 等）
- 配置证据：[config.yaml](https://github.com/xinnan-tech/xiaozhi-esp32-server/blob/main/main/xiaozhi-server/config.yaml)（`selected_module.TTS: EdgeTTS`；`TTS:` 段）

## 4. 对 Nova 的启示

1. 「小智那种好听」≈ 火山引擎豆包大模型 TTS（`volc.service_type.10029`、`*_moon_bigtts`），是**云端付费**能力，不是本地模型；社区里把它当作质量档，默认仍是免费的 Edge TTS。
2. 小智服务端的 TTS 是**可插拔适配层**（`type` 选择 `edge/doubao/aliyun/siliconflow/minimax/...`），和 Nova 已经预留的 `SpeechProvider` + `ApiCapability::Speech` 思路一致。
3. 豆包走的是**双向流式**合成（边生成边回传音频），这既提升自然度也降低首帧延时；若 Nova 要接云端 TTS，应优先选支持流式的接口。
4. 结论与 `docs/tts-options-research.md` 一致：想接近豆包质量，要么接云端同款，要么在本机跑更大的现代模型；Piper（VITS 小模型）到不了这个水平。

## 来源清单

- 78/xiaozhi-esp32 README_zh.md — https://github.com/78/xiaozhi-esp32/blob/main/README_zh.md
- 78/xiaozhi（作者参考后端，已归档）tts-server — https://github.com/78/xiaozhi/tree/main/tts-server
  - bytedance_tts.js — https://github.com/78/xiaozhi/blob/main/tts-server/bytedance_tts.js
  - dashscope_tts.js — https://github.com/78/xiaozhi/blob/main/tts-server/dashscope_tts.js
  - tts-list.js — https://github.com/78/xiaozhi/blob/main/tts-server/tts-list.js
- xinnan-tech/xiaozhi-esp32-server config.yaml — https://github.com/xinnan-tech/xiaozhi-esp32-server/blob/main/main/xiaozhi-server/config.yaml
- xinnan-tech/xiaozhi-esp32-server TTS providers — https://github.com/xinnan-tech/xiaozhi-esp32-server/tree/main/main/xiaozhi-server/core/providers/tts