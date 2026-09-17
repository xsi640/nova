# TASK-015 平台适配与最小回归验证

最后更新：2026-09-18。本记录区分已在真实设备完成的验证、可重复的自动检查，以及尚未在目标平台运行而不能宣称完成的项目。

## 验证范围与判定

目标支持范围是 Windows 10/11 x64、macOS 13+ Intel 和 macOS 13+ Apple Silicon。第一版只验证开发环境直接运行，不生成安装包。每个平台都必须验证：启动、SQLite 打开和重启、系统凭据、麦克风、通知、后台驻留及空闲检测。

“已通过”只表示已经在对应的真实操作系统和硬件上运行并观察到结果；源码审查、单元测试或其他平台的编译均不能替代该验证。

## 当前 macOS 实机记录

本次机器：macOS 14.8.9（23J631）、Intel `x86_64`；Node `v24.21.0`、npm `11.19.0`、Rust/Cargo `1.98.1`。因此它覆盖 macOS 13+ Intel 的开发工具链和本地库路径，但不覆盖 macOS 13 的最小版本，也不覆盖 Apple Silicon。

| 检查 | 命令 | 结果 |
|---|---|---|
| 前端类型检查 | `npm run typecheck` | 通过：TypeScript 无输出、退出码 0。 |
| 格式/补丁空白检查 | `git diff --check` | 通过：无输出、退出码 0。 |
| Rust 单元测试集合 | `npm run test:rust` | 通过：43 项测试通过、退出码 0。 |
| 原生窗口直接运行 | `npm run tauri dev` | 通过编译并启动 `target/debug/nova`；验证进程随后由测试主动停止。尚未完成窗口可见性、Keychain、麦克风、通知、后台驻留或空闲检测的人工观察。 |

macOS 的原生依赖脚本会输出 `Using the operating system SQLite dynamic library.`；这符合 `scripts/prepare-native.mjs` 的非 Windows 路径。数据库初始化和重开有 Rust 单元测试，但该测试结果仍受上表所列的 Cargo 锁影响。

## 源码审查结论

这些结论是实现状态，不是实机验收：

| 能力 | 当前实现/配置 | 实机状态与限制 |
|---|---|---|
| SQLite | `Database::initialize` 在应用数据目录创建 `nova.db`，启用外键并运行迁移；关闭窗口时保存窗口几何信息。非 Windows 通过系统 SQLite 动态库；Windows 通过 SDK 的 `winsqlite3` 导入库。 | 可由 Rust 数据库单测覆盖创建、重开及迁移；仍须在每个目标 OS 的实际应用数据目录验证。 |
| 系统凭据 | `keyring` 的 `v1` 后端以服务名 `app.nova.companion` 保存引用化 API Key；数据库只保存 `secret_ref`。 | 需要分别确认 macOS Keychain 和 Windows Credential Manager 的首次授权、读取及重启后的可用性。 |
| 窗口 | Tauri 主窗口定义了默认与最小尺寸；Rust 切换紧凑/管理模式并恢复尺寸与位置。 | 两个平台均须人工检查多显示器、离屏位置恢复和缩放下的行为。 |
| 麦克风/语音 | Rust 已提供转写与合成命令边界及音频格式/大小校验；是否能采集与播放取决于 WebView 前端和系统权限。 | macOS 需授权麦克风；Windows 需允许“桌面应用访问麦克风”。尚无两端实机权限流验证。 |
| 通知 | 已集成 `tauri-plugin-notification`，并提供受控的 `show_notification` 命令与设置页测试入口。 | 原生通知权限、点击行为和 Windows 开发环境显示差异仍须在目标系统手工验证。 |
| 后台驻留/托盘 | Tauri 默认 capability 含托盘权限，但应用 Rust 代码未创建托盘或关闭后隐藏窗口的行为。 | 未实现，不能验证。 |
| 空闲检测 | 前端仅记录最后活跃时间，并以 2 小时阈值、6 小时冷却和免打扰规则触发主动通知；不采集输入内容或鼠标位置。 | 运行期行为与系统后台状态仍须在目标系统手工验证。 |

Windows SQLite 预处理有明确前置条件：`scripts/prepare-native.mjs` 从 `C:\\Program Files (x86)\\Windows Kits\\10` 查找最新包含 `um\\x64\\winsqlite3.lib` 的 SDK，并复制为项目的 `native/sqlite/windows-x64/lib/sqlite3.lib` 与相应头文件。Windows 工作站缺少该 SDK 或文件时，`npm run deps:native`、`npm run check:rust` 和 `npm run tauri dev` 会失败并提示缺失的具体文件。

## Windows 10/11 x64 验证步骤

在一台装有 Node 24.14.0、npm 11.9.0、Rust 1.98.1 MSVC、Microsoft C++ Build Tools、Windows 10 SDK（含 `winsqlite3.lib`）和 WebView2 的 Windows 10/11 x64 实机上执行：

1. `npm ci`
2. `npm run deps:native` — 应输出 `Prepared precompiled Windows SQLite from SDK ...`。
3. `npm run check` — 记录 TypeScript 与 Rust 测试的退出码。
4. `npm run tauri dev`，首次启动时确认主窗口可见且默认聊天页可用。
5. 完成 API 配置，用测试 Key 保存后退出并重新启动；确认 Key 不显示在 UI、连接测试仍可读取 Credential Manager 的引用。
6. 保存角色、设置及一条聊天消息；关闭并重开应用，确认 `nova.db` 能打开、消息/设置/窗口模式恢复。
7. 配置语音能力，允许 Windows “桌面应用访问麦克风”，完成一次录音、转写确认、语音播放和一次拒绝权限后的文字回退。
8. 在实现通知/驻留/空闲检测后：分别验证通知点击跳转、关闭窗口后的托盘驻留与恢复、免打扰抑制主动通知但不抑制日程提醒，以及仅取得空闲时长而不采集输入内容。

## macOS 验证步骤

在 macOS 13+ Intel 与 macOS 13+ Apple Silicon 各一台实机执行：

1. `npm ci && npm run check && npm run tauri dev`。
2. 确认应用可启动、默认窗口/紧凑窗口切换正常；改变位置和大小，重启后确认恢复且不落在不可见区域。
3. 保存 API Key，在 Keychain Access 中确认条目属于 `app.nova.companion`，重启后执行连接测试并确认 UI 仍只显示脱敏状态。
4. 保存本地消息和设置，重启后确认数据库与迁移正常打开。
5. 在“系统设置 → 隐私与安全性 → 麦克风”允许 Nova，完成录音、确认文本和播放；撤销权限后确认错误可理解且仍能文字聊天。
6. 在通知、后台驻留与空闲检测实现后，分别验证系统通知权限与点击路由、Dock/托盘驻留策略和空闲检测的隐私边界。

## 发布前退出条件

TASK-015 只能在以下全部满足后标记完成：上述自动检查在稳定工作树上通过；Windows 10/11 x64、macOS 13+ Intel、macOS 13+ Apple Silicon 都有实际 `tauri dev` 运行记录；各平台的数据库、凭据、麦克风、通知、后台驻留和空闲检测逐项记录结果；失败项有明确的已知限制或已修复的关联任务。当前通知、后台驻留和空闲检测尚未实现，因此 TASK-015 仍处于未完成状态。
