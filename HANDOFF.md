# Computer-use 项目交接

> **实施后的状态更新（2026-10-17）**：下面原交接内容保留为历史背景，不再代表当前代码状态。当前入口为 [README](README.md)、[实际验证记录](docs/verification.md) 和 [平台矩阵](docs/platforms.md)。工程已按确认的布局分为独立 `native/` Rust 与 `typescript/` Node 子工程，根目录没有两种项目清单。Windows SDK/MCP 自建窗口闭环与 stop/EOF/TTL/真实响应丢失测试已通过；新增固定 host-approved `allowed_apps` profile 的 Windows 冷启动 list/launch/get/activate/observe/click/stop debug/release 验收；Rust 12 项、TypeScript 16 项测试通过。Linux 仅通过 aarch64 cargo check/clippy，macOS 未在原生环境编译；不要把三平台源码当作三平台验收通过。原文中的“只有交接文件、尚未实施”以及旧目录建议已过时。没有提交、推送、发布或修改全局 MCP 配置。

> 更新时间：2026-10-17。供在 `D:\clddup\computer-use` 新建的对话继续使用。
> 这是一份用户要求生成的交接文档，不代表所有建议均已完成或用户已批准实施。

## 1. 用户要做什么

建立一套**独立、跨平台的 computer-use 能力**，供不同 Agent 使用：看桌面、读取界面控件、点击、输入、按键、滚动、拖动，而不是 browser-use。

用户明确要求：

1. 项目位置：`D:\clddup\computer-use`。
2. **Windows、macOS、Linux 三个平台同时建设**，不是先只实现 Windows、其他平台留空壳。
3. 使用开源 **REA（Reverse Engineer Anything）** 分析**本机已安装的 Codex 发行版**，理解实现后独立复刻 computer-use。
4. REA 已确认指的是官方项目：https://github.com/morluto/rea 。
5. 不再以 GitHub 上 ZCode 的公开占位实现作为真实执行器；用户特别指出发行版比公共源码完整。
6. 最终接入其他 Agent，不需要再开发一个含规划模型的新 Agent。
7. 用户有 mise，**已明确授权安装兼容 Node 并修改全局默认版本**，无需坚持隔离 Node 环境。

### 用户原话要点

- “我要的是跨平台的。”
- “我想给其他agent也接入computer-use……自己怎么封装比较好。”
- “别看github开源的zcode……[公共版被删减]。”
- “D:\clddup\computer-use 我准备这里初始化我的项目……使用开源项目rea来分析codex……复刻一个computer-use。”
- 澄清选择：“独立实现，三平台同时建设”。
- “我有mise 你直接安装对应版本node就行 可以修改全局的。”

## 2. 当前进度——不要误报完成

**已完成：**
- 确认目标目录存在且为空，未找到 `D:\CLAUDE.md`、`D:\clddup\CLAUDE.md` 或项目内 CLAUDE.md。
- 对本机 Codex、ZCode 发行文件进行只读静态分析。
- 核实 REA 官方来源、基本运行条件及静态分析入口。
- 编写实施方案，并通过计划工具提交过；工具提示方案展示给用户，但**没有收到用户明确批准整个实施方案**。随后用户要求生成交接文档以新建对话。

**未完成：**
- 尚未初始化项目代码、Git、workspace。
- 尚未安装/运行 REA。
- 尚未用 Ghidra/IDA 反编译 Codex 原生执行器。
- 尚未通过 mise 安装/切换 Node。
- 尚未编译或运行任何独立 computer-use 实现。
- 没有加载 Codex/ZCode 原生模块、启动它们的 helper 或连接其内部 IPC。

当前目录只应包含本次交接文件；不要把此文档理解为已有实现。

## 3. 环境

Windows 11 Pro，原会话工作目录 `D:\Desktop`，新会话应在项目目录打开。

上次实际检查得到：
- Node `v25.9.0`（mise shim：`E:\Software\mise\shims\node.cmd`）。
- pnpm `11.2.2`。
- rustc/cargo `1.90.0`（`D:\Software_Cache\.cargo\bin`）。
- Git 已安装。
- `java`、`ghidraRun`、`analyzeHeadless` 未在 PATH 中发现，**不代表全盘不存在**。
- 找到 dumpbin：`C:\Program Files\Microsoft Visual Studio\2022\Community\VC\Tools\MSVC\14.42.34433\bin\Hostx64\x64\dumpbin.exe`；此前调用退出 157，静态分析改用自写 Python PE 解析器完成，没有为此修改环境。

REA 声明支持 Node `^22.19.0 || ^24.11.0 || >=26.0.0`，Node 25 不在声明范围。
**推荐安装 Node 24 LTS 最新可用补丁并设为 mise 全局默认**；用户已对此授权，实施前核对当前工具解析与 mise 配置。

## 4. 重要概念纠正

- **trycua 的 Cua Driver ≠ 所有名为 CUA 的组件。** CUA 也只是 Computer-Using Agent 的泛称。
- 推荐过 Cua Driver 作为可复用开源底座，但没有依据说 Codex 或 ZCode 官方使用它。
- ZCode-CE 社区版明确用 `@trycua/cua-driver`；ZCode 官方发行版不是同一实现。
- 用户当前方向是**独立实现**，不要擅自退回只封装 trycua 或 Codex SDK 的方案。
- MCP 是 Agent 接入协议，不是 OS 自动化驱动。
- 单独复制 SDK 不等于获得完整运行时；原应用的授权、生命周期、状态处理往往在宿主中。

## 5. 已有完整本机报告与证据

### 主报告

`D:\Desktop\computer-use-local-analysis-20261017.md`

此文件是可复用的本机静态分析总结，含真实文件链接、行号、hash 和事实/未知边界。新会话先读它，不要从头重复广泛搜索。

### Codex 证据（临时目录，可能被系统清理，使用前核验存在）

`C:\Users\cl-ddup\AppData\Local\Temp\codex-static-fvz52uq7\`

关键文件：
- `reports\text-evidence.md`
- `reports\focused-evidence.json`
- `reports\pe-evidence.json`
- `reports\hashes.json`
- `reports\sdk-source-hashes.json`
- `asar-selected-01\_provenance.json`
- `asar-selected-02\_provenance.json`

### ZCode 证据（同样需检查是否仍存在）

`C:\Users\cl-ddup\AppData\Local\Temp\zcode-static-54znojek\`

- `reports\source-hashes.json`
- `reports\ax-native-pe.json`
- `asar-selected-mapping.json`
- `inventory\asar-index.json`

这些来自已停止/结束的子 Agent 工作，不要假设新会话能恢复旧子 Agent 上下文。直接读取所需证据即可。

## 6. 本机 Codex 的真实实现结论

### 安装与版本

安装根目录：
`C:\Program Files\WindowsApps\OpenAI.Codex_26.1013.3656.0_x64__2p2nqsd0c76g0`

- Appx `26.1013.3656.0`。
- 包内应用及 bundled 插件 `26.1013.31256`，build 20586。
- `@oai/sky 0.7.8`、`@oai/cua 0.2.6`、`@oai/cua-repl 0.1.0`。
- 运行时清单 Node `24.21.0-cua.1`。

软件可能自动更新，因此先重新定位版本，不能死依赖旧路径。

### 主链（静态确证）

```text
Agent 的 node_repl JavaScript
  → @oai/sky（公共导出 sky）
  → nodeRepl.rpc("sky", setup / execute)
  → @oai/sky/service.handleRpc
  → WindowsComputerUseClient
  → 可信 nodeRepl.nativePipe bridge
  → Electron 本地命名管道
  → WindowsHelperTransport
  → Rust codex-computer-use.exe
  → Windows UI Automation / SendInput / Graphics Capture
```

SDK 根目录：
`<安装根>\app\resources\cua_node\bin\node_modules\@oai\sky`

插件文档：
- `<安装根>\app\resources\plugins\openai-bundled\plugins\computer-use\skills\computer-use\SKILL.md`，18–29 行是 sky 初始化入口。
- 同插件 `docs\api.md`，13–28 行为 Windows window2 接口；54–74 行为观察/点击参数；122–138 行为状态/截图结构。

核心 API：
`list_apps/list_windows/get_window/launch_app/get_window_state/click/press_key/type_text/scroll/set_value/drag/perform_secondary_action/activate_window`。

关键设计：
- 动作绑定 Window `{app,id,title?}`。
- 观察可返回 accessibility tree 和 screenshots[]，默认截图开启、文本关闭。
- 元素索引来自最新观察；坐标是窗口相对坐标，可关联 screenshotId。
- 图片可能有多个 bounded region、origin、尺寸、zIndex，不要假定总是单张全屏图片。
- SDK/宿主可以将图片作为图像内容投递给模型。

两层传输不同：
- Electron 管道：4 字节小端长度 + UTF-8 JSON-RPC 2.0。
- 原生 helper：stdin/stdout NDJSON，请求/响应带 ID，支持错误和审批交互。

原生证据：
- Rust helper 导入 `SendInput`、`D3D11CreateDevice`、`uiautomationcore.dll`、`RoGetActivationFactory`。
- 含 `Windows.Graphics.Capture.GraphicsCaptureItem`、`Direct3D11CaptureFramePool`。
- 另有 Swift helper 被 Appshot 子系统选择，不是可以一概忽略的废文件。

**未知与限制：**
- SDK 有普通 Node 创建 client 的分支，但 helper 仍有账号/app-server、feature policy、应用审批依赖。
- 未验证脱离 Codex 独立运行。
- 插件 Proprietary，不能把可读安装文件视为可自由分发代码。
- 不通过设置内部审批字段、伪装宿主或直连现有管道来绕过限制。

文件指纹：
- Codex app.asar SHA256 `163a18c84db57ad5c50222b313dc4d11d45febc03ec065b516ba9f0380f2eb3f`。
- sky Rust helper SHA256 `14d5e9c7336dd34aa8476f218dd45e033b8416bdad6c58fa79c84169ae59f2f3`。

## 7. ZCode 发行版的参考事实（次要，不替代 Codex 主线）

本机 `E:\Software\ZCode`，版本 3.14.4；producer 0.6.3，清单记录 Electron 41.0.3。

- `resources\glm\packages\zcode-cua-plugin\scripts\computer-use-client.mjs`：888–928 行检查 host bridge，并区分可重试的 not-ready 和不能重放的 possibly-sent 动作。
- `resources\glm\packages\node-repl-host\dist\mcp\server.js`：80503 行起内嵌真实 `@zcode/zcode-cua/dist/index.js`；115956–116062 行真实 runtime；117217–117319 行令牌 broker。
- `resources\tools\cua-helper\dist\windows-helper.js`：14758–14780 行加载原生扩展；22720–22802 行 UIA capture、验证窗口和 PNG、元素动作。
- `resources\tools\cua-helper\build\Release\ax_native.node`：x64，N-API 7；UIA/SendInput/WGC 原生路径。

架构：SDK → Worker bridge → 带随机令牌的 host broker → producer（会话/状态/动作策略）→ helper broker → ax_native.node。

中间有 AccessibilitySession、KillSwitch、InputHoldRegistry、ActionSettler，不只是点击包装。

尚未定位该发行版 desktop 到 Windows helper 的完整 launcher，不能声称默认启用/独立可用。发现过本地 peer 校验放宽的静态分支，但**没有验证运行态或尝试利用**，不要把它当正式开放接口。

## 8. REA 使用方案

官方： https://github.com/morluto/rea

核查时 `rea-agents` package 6.5.0，MIT；实际使用时固定并记录核验后的版本、来源及依赖条款。

已查明：
- JS/Electron 静态分析能直接处理 app 目录或 ASAR，不必先装原生反编译器。
- 文档示例入口是 `analyze-javascript-application <absolute-path> --json`（从固定版本的 rea-agents 调用，执行前确认该版本 CLI）。
- `npx rea-agents setup` 会修改 Agent/MCP 配置，可能提示安装工具；**不要不经核查直接全局 setup**。
- 二进制深度分析需 Ghidra/IDA/Hopper；Windows Ghidra 集成标为实验性。
- 分析运行本地，返回给 Agent 的片段经过当前模型服务，不应说完全离线。

建议研究顺序：
1. SDK/JS 静态分析：明确窗口/状态/动作/图像输出契约。
2. 原生定向分析：窗口身份、UIA 元素定位、WGC 坐标/DPI、输入与取消、可能已投递语义。
3. 将研究结果整理成**独立功能规格**，再写自己的实现，不逐行翻译专有反编译代码。

不要无限逆向：已有可读 JS 和公开 OS API 足够解释的部分不重复钻原生代码。

## 9. 待确认的实施方案（已提交，未获整体验收式批准）

原计划文件：
`C:\Users\cl-ddup\.claude\plans\curried-hopping-globe.md`

若原会话计划文件不可用，本节足以继续讨论/落地。新会话先简短确认执行范围，不要把“写交接文档”当作整个实施方案的批准。

### 推荐架构

pnpm TypeScript workspace + Rust workspace：

```text
任意 Agent
  ├─ MCP stdio
  └─ TypeScript SDK
       ↓
Computer Runtime
  ├─ 会话、应用授权、跨进程桌面控制租约
  ├─ 观察/元素/图片映射
  ├─ 动作状态、取消、停止、输入释放
  └─ 版本化 helper 协议
       ↓
独立 Rust helper
  ├─ Windows backend
  ├─ macOS backend
  └─ Linux backend
```

不内置 LLM，不依赖 Codex/ZCode 私有组件执行，不开放公网端口，不暴露任意宿主 JS 执行。

### 目录建议

```text
packages/protocol/     # 共享 schema / 类型
packages/runtime/      # 会话、policy、lease、observations、driver client
packages/mcp/          # MCP stdio
packages/sdk/          # Agent 无关 TS SDK
crates/protocol/
crates/core/
crates/helper/
crates/platform-windows/
crates/platform-macos/
crates/platform-linux/
docs/research/         # 脱敏事实、来源、行为规格
skills/computer-use/
tests/fixtures/        # 自建测试，不含专有目标代码
```

### 统一工具和行为

首批：`computer_capabilities`、`computer_start`、`computer_list_windows`、`computer_observe`、`computer_act`、`computer_stop`。

- observe 返回 observation ID、window ref、element refs、可选图片和坐标映射。
- act 绑定当前窗口和观察；图片坐标由 runtime 映射 OS 坐标，不让模型算 DPI。
- 失效观察、窗口替换返回明确错误。
- rejected/dispatched/verified/unknown 分开；unknown 不自动重试。
- request ID 去重不等于 OS 恰好执行一次。
- 控制租约跨进程生效；不同 MCP 进程不能同时抢一个桌面。
- 模型不能自授权限；停止优先于排队动作，清理按住的键/鼠标。
- 图像以客户端能送入视觉模型的 image 内容返回，不只返回本地路径。
- 同一协议驱动 MCP 与 SDK，TS/Rust 类型生成或一致性验证，避免漂移。

### 三平台同步建设

各平台都实现真实最小闭环：枚举窗口 → 观察 → 点击/输入/按键 → 重观察 → 停止。不能用 mock/stub 冒充支持。

- Windows：UI Automation、WGC/D3D11、SendInput；处理 HWND/PID、DPI、多屏、提升权限。
- macOS：AX、ScreenCaptureKit、CoreGraphics；明确 Accessibility/Screen Recording 授权。必要时薄 Swift bridge，接口不变。
- Linux：AT-SPI + X11 真实后端；Wayland 按 compositor/portal 实际能力限定，需交互授权；不支持就明确返回，不承诺任意窗口/后台输入。不能仅有 Linux 包名就声称 Wayland 完整。

**只有 Windows 本机可实测**。macOS/Linux 代码同时建设，但编译、真实 GUI 运行和权限验证分开报告；缺测试环境如实列待验证，不把 CI 构建当 E2E。

### 实施里程碑

1. mise Node 切换；初始化 workspace、依赖锁、测试入口。
2. REA 固定版本审查与定向分析；产出 `docs/research/codex-behavior.md`、`provenance.md`。
3. 独立协议、runtime、三平台 helper 最小纵向能力。
4. MCP/SDK 和 skill；可靠性与权限/停止测试。
5. Windows 无敏感测试应用跑通；其他系统明确测试矩阵和后续运行方式。

尚未选定具体 crate 版本，不要根据建议架构假称所有依赖已验证。

## 10. 安全、授权与工程边界

- 用户授权研究自己安装的发行版，目标是互操作理解与独立实现；不绕过登录/权益/审批/身份校验，不复制专有实现分发。
- 原生目标和下载归档都按不可信数据处理：各自新建空目录；自写脚本在不同目录；Python `-I`；不从目标/提取目录执行解释器或构建工具。
- REA 工具安装与任何 runtime/instrumentation 是不同动作；现阶段只计划静态分析，不擅自启动 Frida/注入目标。
- 不读取聊天、cookies、账号 token 或凭据，不把目标安装文件上传外部服务。
- Node 全局切换已授权；其他未明确授权的全局 Agent 配置修改、提权、大型反编译工具安装、发布/推送、付费调用仍需按实际影响处理。
- 若修改 Claude settings，按可用 update-config skill；不要通过记忆假装安装了 MCP。
- 不自动提交或推送 Git，不建立外部仓库，除非用户要求。

## 11. 验证清单

- 协议和类型、参数校验、帧上限、超时、版本不兼容。
- 观察过期、窗口替换、缩放/负屏幕原点、元素失效。
- 跨进程控制互斥、去重、unknown 不重放。
- 权限拒绝、stop 抢占、断线清理、键鼠释放。
- 自建测试应用：读取 → 聚焦 → 中英文输入 → 检查结果 → 停止。
- MCP 工具发现、图像返回、结构化错误；SDK 和 MCP 使用同一 runtime。
- 各系统结果分别记为静态检查/编译/GUI实测，绝不混写。

## 12. 新对话建议开场

用户可以直接发送：

> 请先阅读 HANDOFF.md。继续这个独立跨平台 computer-use 项目：使用 REA 分析本机 Codex，Windows/macOS/Linux 同时建设。Node 可用 mise 安装并修改全局默认。先确认交接中方案和实际环境，再开始实施，不要重复已经完成的静态广泛调研。

新 Agent 应从读取本文件和桌面主报告开始，核验安装版本与临时证据可用性，列出真实阻塞。不要把旧 Agent 的后台任务视为仍在运行，也不要在没有实现/测试时宣称已跑通。
