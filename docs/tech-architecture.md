# 阶段 3A：技术架构

## 1. 架构目标与约束

本架构服务于单用户、本地优先的 Windows/macOS AI 虚拟女友桌面应用，目标是在不建设服务端、不支持云同步和不运行本地 AI 模型的前提下，完成长期对话、语音交互、记忆、主动陪伴和应用内日程。

架构约束：

- 支持 Windows 10/11 x64，以及 macOS 13 及以上版本的 Apple Silicon 与 Intel 设备。
- React 负责界面；Rust 负责本地能力、数据和所有远端 API 调用。
- 聊天、记忆、日程、角色设定和应用设置仅保存于本机 SQLite 数据库；第一版不设置数据库密码，也不加密数据库文件。
- API Key 可在设置页配置、修改和测试，但明文只能保存在操作系统凭据存储中。
- 低频输入检测只产生空闲状态和持续时间，不采集键盘内容、鼠标位置或具体操作。
- 对话、语音识别和语音合成均支持独立的 OpenAI 兼容 API 配置，且可复用同一 API Key。
- 第一版只要求开发环境直接运行，不产出安装包。

## 2. 交付形态与运行环境

交付物是单机 Tauri 桌面应用，无自建后端、无账号体系、无多设备同步。

| 项目 | 约定 |
|---|---|
| Windows 运行环境 | Windows 10/11 x64、WebView2、Rust MSVC 工具链 |
| macOS 运行环境 | macOS 13+、Apple Silicon 或 Intel、Xcode Command Line Tools |
| 前端运行环境 | Node.js 24.14.0、npm 11.9.0 |
| 本地启动方式 | `npm run tauri dev` |
| 远端依赖 | 用户自行配置的 OpenAI 兼容 API |
| 数据位置 | 应用数据目录中的明文 SQLite 数据库 |

Windows 开发依赖 Microsoft C++ Build Tools 与 WebView2；macOS 开发需要 Xcode Command Line Tools。Tauri 官方前置条件说明了这些平台依赖。[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)

## 3. 技术栈与分层

### 技术栈

| 类别 | 技术与版本 | 用途 |
|---|---|---|
| 桌面框架 | Tauri 2.11.5；@tauri-apps/cli 2.11.4；@tauri-apps/api 2.11.1 | 桌面窗口、Tauri 命令、系统能力桥接 |
| 前端 | React 19.3.0；React DOM 19.3.0；TypeScript 7.0.2；Vite 8.3.0 | 页面、交互、状态与开发构建 |
| Rust 运行时 | Rust 1.98.1 | 本地业务服务与平台能力 |
| 数据库访问 | rusqlite 0.40.2 | Rust 仓储层与 SQL 访问 |
| 本地数据库 | 操作系统提供的 SQLite；rusqlite 0.40.2 | 本地关系数据存储；数据库文件不加密 |
| 系统凭据 | keyring 4.2.0 | Windows Credential Manager 与 macOS Keychain 访问 |
| 通知 | tauri-plugin-notifications 0.4.6 | 系统通知与权限处理 |

Tauri 2.11.5 以 Rust 后端与 WebView 前端构成桌面应用；React 19.3 是当前稳定版本；Rust 通过 rusqlite 访问操作系统提供的 SQLite。[Tauri](https://docs.rs/tauri/latest/x86_64-pc-windows-msvc/tauri/)、[React 19.3](https://react.dev/blog/2026/09/09/react-19-3)

### 分层

```text
React UI
  └─ Tauri 命令客户端
       └─ Rust 应用服务
            ├─ 领域仓储与 SQLite 数据库
            ├─ API Key 系统凭据管理
            ├─ OpenAI 兼容 API 适配器
            └─ Windows/macOS 平台适配器
```

- **React UI 层**：实现已确认的聊天浮窗、首次引导、记忆、日程和设置页面；不保存 API Key，不直接访问数据库或远端 API。
- **Tauri 命令边界**：仅暴露业务命令和受控应用事件，不将数据库连接、系统密钥或 HTTP 客户端泄露给前端。
- **Rust 应用服务层**：编排对话、语音、记忆、日程、主动陪伴、设置和导出流程。
- **基础设施与平台适配层**：实现 SQLite、系统凭据、HTTP、系统空闲时长、托盘、窗口与通知。

## 4. 模块划分与需求模块映射

| 代码模块 | 需求模块 | 前端职责 | Rust 职责 |
|---|---|---|---|
| MODULE-001 | 虚拟女友设定 | 首次设定与设置页 | 保存、读取和校验姓名、性格、说话方式 |
| MODULE-002 | 对话交互 | 长期聊天页、文字输入、语音识别确认、语音播放 | 保存消息、调用对话 API、调用识别与合成 API |
| MODULE-003 | 长期记忆 | 记忆列表、来源消息、编辑、删除、导出 | 自动提取候选记忆、关联来源消息、持久化与查询 |
| MODULE-004 | 主动陪伴 | 主动消息在聊天时间线中的展示 | 只检测空闲时长、检查免打扰、生成与保存消息、触发通知 |
| MODULE-005 | 日程管理 | 聊天确认卡、日程列表与编辑删除入口 | 解析日程意图、校验确认卡、日程增删改查和提醒调度 |
| MODULE-006 | 本地数据 | 数据管理与导出入口 | SQLite 数据库、迁移、导出、删除和恢复错误 |
| MODULE-007 | 桌面运行与通知 | 浮窗与通知点击后的页面响应 | 窗口、托盘、后台驻留、平台空闲检测与系统通知 |
| MODULE-008 | AI 服务配置 | API 表单、测试和脱敏状态 | 保存非敏感资料、写入或读取凭据、连接测试、API 适配 |

模块边界：

- MODULE-003 只管理记忆，不决定主动消息的触发时机。
- MODULE-004 只决定何时主动参与，不创建或修改日程。
- MODULE-005 只管理应用内日程，不读取系统日历。
- MODULE-006 保存业务数据；MODULE-008 保存 API 配置引用和管理密钥，二者不重叠。
- MODULE-007 仅暴露抽象的平台能力；业务规则仍由对应应用服务模块维护。

## 5. 数据与存储

### 数据实体

| 实体 | 关键字段 | 说明 |
|---|---|---|
| persona_profile | id、name、personality、speech_style、updated_at | 当前虚拟女友设定 |
| api_profiles | id、capability、base_url、model、secret_ref、enabled | 对话、语音识别、语音合成三类服务的非敏感配置 |
| chat_messages | id、role、content、audio_ref、created_at、status | 单一连续聊天时间线中的消息 |
| memories | id、content、source_message_id、created_at、updated_at | 自动形成且可人工管理的长期记忆 |
| schedules | id、title、scheduled_at、remind_at、source_message_id、status | 应用内日程及其提醒状态 |
| proactive_events | id、message_id、idle_started_at、notified_at、opened_at | 主动陪伴消息与通知状态 |
| app_settings | theme、dark_mode、dnd_start、dnd_end、voice_autoplay | 非敏感应用行为设置 |

### 本地存储与密钥策略

- 使用操作系统提供的 SQLite 创建本地数据库，不设置数据库密码，也不加密数据库文件。
- API Key 由 MODULE-008 写入系统凭据存储；`api_profiles.secret_ref` 只保存可定位密钥的引用。
- React 读取 API 配置时只能得到脱敏状态和元信息，不能得到 API Key。
- 数据库文件可能被拥有本机文件访问权限的其他程序直接读取；第一版接受该风险。
- 导出在 Rust 中生成；导出文件的格式在任务设计阶段确定。

### 自动记忆与日程解析

- 对话消息先持久化，再由 Rust 服务发起后台解析。
- 记忆解析产出受限的候选记忆结构；校验失败时不写入记忆。
- 日程解析只生成待确认日程数据；用户在聊天确认卡确认前不写入 `schedules`。
- 自动解析的 API 失败不影响已保存的聊天消息，前端收到可理解的状态反馈。

## 6. 接口约定

### 本地命令

React 只能通过 Tauri 命令调用 Rust 服务。命令以模块为边界，不暴露 SQL 或密钥。

| 类型 | 本地命令类别 | 对应模块 | 用途 |
|---|---|---|---|
| 本地命令 | bootstrap、get_onboarding_status | MODULE-001、MODULE-008 | 读取首次启动状态 |
| 本地命令 | save_persona、get_persona | MODULE-001 | 管理虚拟女友设定 |
| 本地命令 | send_message、transcribe_audio、synthesize_speech | MODULE-002 | 文字与语音交互 |
| 本地命令 | list_memories、update_memory、delete_memory、export_data | MODULE-003、MODULE-006 | 管理与导出数据 |
| 本地命令 | confirm_schedule、list_schedules、update_schedule、delete_schedule | MODULE-005 | 管理应用内日程 |
| 本地命令 | save_api_profile、test_api_profile、get_api_profile_status | MODULE-008 | 配置、测试和读取脱敏 API 状态 |
| 本地命令 | save_settings、get_settings | MODULE-004、MODULE-007 | 管理免打扰、主题和语音播放设置 |

### 远端 API 适配

| 能力 | 默认兼容接口 | 配置项 |
|---|---|---|
| 对话 | `POST /chat/completions` | base URL、路径、模型、API Key 引用 |
| 语音识别 | `POST /audio/transcriptions` | base URL、路径、模型、API Key 引用 |
| 语音合成 | `POST /audio/speech` | base URL、路径、模型、API Key 引用 |

- 三类能力各有配置资料，允许它们指向同一供应商或不同供应商。
- 远端请求统一由 Rust 发送，并使用 `Authorization: Bearer <API Key>`。
- API 连接测试不在日志中记录 API Key。
- 统一错误模型包含：配置错误、网络错误、授权错误、服务响应错误、音频错误、数据库错误和平台权限错误。

## 7. 构建、运行与部署

- 源码在 Windows 使用 Windows Rust MSVC 工具链运行，在 macOS 使用 Xcode Command Line Tools 运行。
- 开发时执行 `npm install` 后使用 `npm run tauri dev` 启动。
- 前端开发构建由 Vite 处理；Rust 编译和应用窗口由 Tauri 处理。
- 第一版只验证直接运行，不生成 MSI、DMG 或签名安装包。
- 应用启动时直接打开应用数据目录中的 SQLite 数据库并执行迁移。

## 8. 架构决策记录

| 编号 | 决策点 | 背景 | 最终选择 | 被放弃的方案 | 放弃原因 |
|---|---|---|---|---|---|
| ADR-001 | 桌面框架 | 需要 React、Rust、后台与原生能力 | Tauri + React + Rust | Electron；Web + 本地辅助程序 | Electron 资源负担更高；Web 边界和部署更复杂 |
| ADR-002 | 数据存储 | 需要长期查询、编辑、关联和导出 | 操作系统 SQLite，第一版不加密 | SQLCipher；JSON 文件 | 用户决定第一版暂不加密；JSON 不适合关系查询、迁移和稳定编辑 |
| ADR-003 | API Key 保存 | API Key 需在设置页配置且不能明文保存 | 系统凭据存储，设置页脱敏展示 | 明文配置文件；普通 SQLite 字段 | 明文或普通字段的泄露风险更高 |
| ADR-004 | 远端 API 调用位置 | React 不应持有 API Key | Rust 服务层统一调用 | React 直连远端 API | 会暴露密钥并导致错误处理分散 |
| ADR-005 | 主动陪伴输入检测 | 需要判断低频操作且保护隐私 | 仅采集空闲状态与持续时间 | 键盘监听内容；鼠标轨迹采集 | 超出需求且侵犯隐私 |
| ADR-006 | 服务端与同步 | 第一版为单用户、本地使用 | 不建设服务端、账号与云同步 | 自建云端后端 | 超出当前范围并增加维护成本 |
| ADR-007 | 语音与对话配置 | 三类模型能力可能来自不同兼容服务 | 独立配置资料，可复用同一 Key | 强制单一服务配置 | 限制兼容供应商与模型选择 |

## 9. 技术风险

- OpenAI 兼容服务对聊天、语音识别和语音合成端点的兼容程度不同；连接测试必须按能力分别执行。
- 明文 SQLite 数据库包含敏感对话信息；拥有本机文件访问权限的其他程序可能直接读取数据。
- Windows/macOS 的空闲时长检测、通知权限和凭据存储 API 不同，应保持在 MODULE-007 与 MODULE-008 的平台适配边界内。
- 外部 API 不可用会影响对话与语音能力；本地聊天、记忆、日程和设置必须仍可读取。
- 自动记忆的误判、重复和不应保存内容是产品风险；架构只保证可追溯、可编辑、可删除，不擅自扩展产品策略。
- 危机情绪的识别和应答话术仍属于已记录的后续产品讨论事项，不在架构层增加未确认的安全功能。

## 10. 待确认事项处理结果

| 编号 | 待确认事项 | 影响范围 | 处理方式 | 最终结论 |
|---|---|---|---|---|
| ARCH-TBD-001 | 桌面框架与前后端技术栈 | 全局架构 | 确认 | 采用 Tauri + React + Rust |
| ARCH-TBD-002 | 支持系统范围 | 构建与运行 | 确认 | Windows 10/11 x64；macOS 13+ Apple Silicon/Intel |
| ARCH-TBD-003 | API Key 配置与保存方式 | API 安全 | 确认 | 设置页可配置；明文仅保存在系统凭据存储；Rust 统一调用远端 API |
| ARCH-TBD-004 | 聊天与业务数据存储方式 | 数据持久化 | 确认 | 使用明文 SQLite，不设置数据库密码；后续可重新评估加密迁移 |
| ARCH-TBD-005 | 低频输入的采集范围 | 主动陪伴、隐私 | 确认 | 仅使用空闲状态和持续时间，不采集输入内容或位置 |
| ARCH-TBD-006 | 对话、识别、合成的 API 配置粒度 | AI 服务配置 | 确认 | 三类能力独立配置，允许复用同一 API Key |
| ARCH-TBD-007 | 第一版部署形态 | 构建与发布 | 确认 | 只要求开发环境直接运行，不构建安装包 |
| ARCH-TBD-008 | 数据导出文件格式 | 数据管理 | 延后 | 任务设计阶段确定；架构保留 Rust 导出服务边界 |
| ARCH-TBD-009 | 危机情绪识别与话术细则 | 对话与产品安全 | 延后 | 延续阶段 1 结论，后续产品讨论；架构不增加未确认功能 |

## 11. 人工确认结论

- 用户已确认 Tauri + React + Rust 方案，且明确要求当前阶段不进行编码。
- 用户已确认对话、语音识别和语音合成采用可配置的 OpenAI 兼容 API。
- 用户已确认 API Key 继续由设置页配置；系统凭据存储负责保存密钥，Rust 服务层负责实际调用。
- 用户已确认 Windows 10/11 x64 与 macOS 13+ Apple Silicon/Intel 支持范围。
- 用户已确认第一版使用明文 SQLite，不设置数据库密码。
- 用户已确认低频输入检测不采集键鼠实际内容或位置。
- 架构已覆盖需求文档的全部模块，并与已确认的 UX 页面、后台主动陪伴、免打扰、日程确认和本地数据边界保持一致。
- 阶段 3A 完成。根据用户要求，本次仅产出架构文档，不进入阶段 3B 的任务拆解或代码实现。
