# 实际验证记录

日期：2026-10-23（含 2026-10-17/18/19/22 基线）。此处区分源码实现、类型检查、单测和真实桌面测试，不把 CI 配置或 cfg 排除路径当作平台验证。

## 2026-10-23 仓库内 release 打包

- `pnpm --dir typescript package:macos` 已实际生成并重复更新 `dist/app-use/`，包括 Cargo release helper、Swift bridge、MCP 及生产依赖。Cargo 报告 `opt_level=3`、`debug_assertions=false`；包内 helper 的 rpath 只保留 `@executable_path`，签名检查通过，默认入口不会在包损坏时回退到 debug。
- 打包脚本在移动暂存目录后执行真实 MCP 初始化、11 个工具发现、native capabilities 及 stop，再替换旧包；不启动桌面会话或请求权限。另将完整包复制到带空格的新位置验证，dyld 实际加载该位置的 sibling bridge，未使用 Cargo build cache。记录：`.local/package-macos/relocated-deNuh8/results.json`。
- TypeScript 构建、协议一致性、typecheck、**56 项测试**通过；新增 6 项覆盖 release 产物选择、rpath、目录保护、依赖链接边界及包内 helper 解析。输出产物被 Git 忽略，脚本不会修改用户配置。
- 已单独备份并将本机用户级 `app-use` 的入口与 helper 切换为 `dist/app-use/` 中的路径，保留原环境及其他配置；没有终止已有客户端，需重连加载新包。
- **权限与分发边界**：当前包使用显式 ad-hoc 签名，非 Developer ID、未做公证，不保证重编译后 TCC 授权沿用。新 helper 的 AX、屏幕录制预检均为 false，因此本轮不运行会请求权限的 GUI 测试，不将 native MCP 连接通过冒充已获系统授权或已通过 release GUI 验收。未打开系统设置、重置 TCC 或改走全局 AppleScript 输入。

## 2026-10-23 WPS 后台输入故障复验

- 会话 `6cf02aa4-110e-4160-892b-30e352cf62cf` 已使用 `screencapturekit-stream`，不是旧桌面截图路径。普通定向点击虽返回 dispatched，WPS 新建浮层未出现；最后 `cmd` 不受支持却被记为 unknown，不能据此断言键盘已执行。
- 对照本机 ChatGPT 原生符号与有界反汇编，补齐仅向目标 PID 发送的 AppKit activation、CPS key-focus-return 通知。WPS 从相同首页状态复现普通点击无效、补齐通知后浮层出现；原前台应用不变。切换同应用窗口保留合成焦点，切换应用及 stop/EOF 清理；用户已经真正激活目标时不发送合成反激活。不新增全局 HID 或隐式 raise/activate。
- 显示宿主设为 accessory，不调用会引入启动激活副作用的 `finishLaunching`。共享标志实测为目标进程的 layer-0 `AXDialog`，唯一子按钮名为 `WindowSharingSessionButton`；按这一语义排除，不凭窗口尺寸、标题或 layer 猜测，也不隐藏真正的新建浮层。修复后实时枚举保留 WPS 文档、排除系统标志。
- **真实 WPS 输入已验证**：通过项目 MCP 选择“文字”、创建 `文字文稿1`，输入“自我介绍”和完整中文段落；另用 CUA 只读窗口截图核实正文实际显示，状态栏字数 63。输入前后前台 PID 均为 1458；测试文档未保存、未上传，未读改已有文件。记录在 `.local/wps-6cf02/`，不提交个人应用截图或会话日志。
- **录屏边界**：本测试宿主跨进程录屏预检仍为 false，WPS 输入验收使用 AX-only policy；CUA 截图只核实输入结果，不冒充项目的 WPS 跨进程取帧测试。项目自身取帧由一个自建后台窗口验证，`.local/macos-shared-capture-bCkQf1/results.json` 通过，红/蓝 PNG 已打开核对。测试移除自身自动激活的启动方式，并对异步合成后的像素更新作有界只读等待；此前失败不算通过。
- **回归通过**：Rust 46 项、TypeScript 50 项、Swift 无输入检查 10 组；单应用后台真实 MCP 含 layer-24 浮层 12 组，`.local/macos-targeted-RtLTvR/results.json`。新增不支持的键在注入前 rejected、`cmd` 别名、应用内部激活及 stop 后反激活断言。异步鼠标采样仍可能包含人工移动，不将其冒充全程硬件隔离证明。Clippy `-D warnings`、fmt、构建、协议一致性及 typecheck 通过。
- 本次未重跑 Windows/Linux GUI，未改变系统权限、全局 MCP 名称或配置。已运行的旧 helper 不会热载入 bridge，客户端需要重新连接 `app-use`。

## 2026-10-23 解锁后复验：共享帧与单应用 MCP

- **真实窗口共享帧通过**：`.local/macos-shared-capture-z9Sc58/results.json`。只有一个自建后台窗口，PNG 为窗口自身的 720×544 画面，不含桌面、Dock 或上层应用。红→蓝的像素更新、4 次静态重读、取消不返回图片、幂等停止与系统共享状态断言通过；`shared-red.png`、`shared-blue.png` 已实际打开核对。系统紫色共享标志确实位于该窗口标题栏。
- 首轮 `.local/macos-shared-capture-lUmVWC/` 的像素已正确，但测试程序用 regular 启动方式成为前台，整体记为失败。修正测试宿主为 accessory，并增加截图前非前台断言后才通过。成功样本仍记录 `foreground_unchanged_sample=false`，不把人工切应用期间的两个时间点宣称为严格的全程前台保位证明，也不承诺所有应用完全隔离。
- **单应用后台真实 MCP：10 组通过**，`.local/macos-targeted-o1Ep1y/results.json`。后台冷启动、点击、Unicode/快捷键、双击/释放、拖动/释放、滚动之外，新增在同一 MCP 连接 stop → start 获得不同原生 helper epoch、拒绝旧观察且不投递输入、新会话重新观察成功。测试仅 AX，不请求屏幕录制；只有一个自建应用，无前台 sentinel，结束后清理。
- MCP 异步样本出现人工移动鼠标，原始采样保留，**不当成鼠标硬件隔离通过证据**。较晚一次额外共享流复跑 `.local/macos-shared-capture-vJqlTd/` 又遇锁屏，正确 blocked；不覆盖上述解锁期间的成功记录。
- 以上不等于 WPS 整个任务完成，也不等于跨进程录屏已验收。当前执行宿主的录屏预检仍为 false，自窗口共享 API 的成功不扩大权限；WPS 新建浮层→空白 Word 仍需在获系统授权的宿主里完整复验。`app-use` 配置未修改，现有 MCP 进程需重连才能载入上一轮构建的新代码。

## 2026-10-23 会话 14725e9d：共享流截图与 MCP 会话恢复

用户指定会话先真实调用 `app-use`，随后在 12:18:35 返回 cancelled；12:18:29 的系统日志显示原 helper 与 ReplayKit 的 XPC 连接中断。后续恢复会话在 12:35:39 出现停流错误 -3808，又出现 capture_timeout，Agent 转而执行 `screencapture -x /tmp/s6.png` 等整屏截图。这些是会话记录中的 Bash 操作，不是 MCP 图片。证据仅保存于 `.local/session-14725e9d/`，不提交个人截图/会话记录。

- 实现：macOS 截图改为长驻 helper 持有的窗口 `SCStream` 帧，AX worker 不再截图；删除 `SCScreenshotManager` 路径，图片标记 `screencapturekit-stream`。流的暂停/空白/中断不返回旧图或桌面图；保留错误 domain/code，区分用户停止与系统故障。已停止流的 -3808 清理作为幂等成功处理。
- MCP 新会话容器支持显式 start → stop → start，以及取消后显式 start；仅清理成功后创建新 helper，阻止并发替换和旧请求污染新会话，不重放动作。SDK 实例终态不变。Agent instructions/skill 明确禁止截图错误后使用 shell/AppleScript/激活窗口绕行；这不是对其它宿主工具的强制限制。
- 静态/协议回归：Rust **45** 项，TypeScript **50** 项，Swift 无输入检查 **9** 组；包含真实 MCP stdio、TS runtime 和两个协议 fixture 子进程的 stop/start 测试，**不是原生 GUI 验收**。共享帧像素编码、idle 复用、suspended 清图和中断失效使用合成 CMSampleBuffer 测试，无屏幕访问。Clippy `-D warnings`、fmt、协议一致性、typecheck 和构建通过。
- **真实 GUI 尚未通过**：初次自窗口共享只收到 suspended；后台 fixture 的 CG 窗口几何与 AX 不一致。随后只读确认系统锁屏标志为 true、前台为 `com.apple.loginwindow`，未发送测试输入、未解锁、未请求或修改系统权限。新增明确的 `desktop_unavailable` 保护。`.local/macos-shared-capture-FuvGSd/results.json` 为 **blocked / exit 2**，不是成功证据。较早测试失败产物保留，未算通过。
- 最终单应用 MCP 回归 `.local/macos-targeted-9EmvJv/results.json` 同样 **blocked / exit 2**：start 阶段已明确提示锁屏，在启动 fixture 前停止。待用户解锁后，顺序重跑 `test:gui:macos:shared-capture` 与单应用 `test:gui:macos:targeted`（新增真实 native stop/start 及旧观察拒绝）。当前测试宿主录屏预检仍为 false；跨进程截图及 WPS 从新建浮层到空白 Word 的完整链路仍需在获系统授权的宿主中验收。旧 MCP 进程不会热载入新版本，需要用户重连 `app-use`。

## 2026-10-23 macOS 后台输入、浮层与系统共享标识

默认桌面会话无需预填 PID；macOS 通过 `NSWorkspace` 打开 bundle ID / `.app`，`activates=false`。正常输入不再隐式激活窗口，只有显式 `activate_window` 会执行激活。输入使用窗口定向 `NSEvent`/`CGEvent.postToPid`，独立指针为非激活、鼠标穿透窗口，不回退全局 HID。非公开 `CGEventSetWindowLocation` 与窗口编号字段做运行时存在性/回读检查，不能保证所有 macOS 版本或应用兼容。

- **单应用后台真实 MCP：8 组通过**，`.local/macos-targeted-7f8KTh/results.json`。仅一个自建应用，由 `computer_launch_app` 冷启动，不预配 PID、不另起前台 sentinel；验证启动/点击不激活且不抬到最上层、Unicode/快捷键、双击、拖动/释放、滚动内容变化、stop。测试 policy 仅为关闭录屏请求而使用 AX-only desktop 模式，不写入用户 MCP 配置。默认不打开浮层演示；`--popup` 才显式执行该项。
- **浮层闭环已通过**，此前 `.local/macos-targeted-nlsRkz/results.json`：自建 `layer 24` 浮层被枚举、AX 观察和点击，主窗口观察会警告前方浮层；不再因输入前 raise 父窗口而将浮层关掉。这次较早的测试用了两个合成应用，后按用户要求撤掉前台 sentinel，后续只运行上述单应用后台测试。
- **系统标识与指针显示专项通过**，`.local/macos-presentation-dcDUQG/results.json`：本进程自建窗口共享，无录屏授权请求，系统 `hasActiveWindowSharingSession` 及紫色标题栏标记随真实 `SCStream` 开/关；独立指针可见、穿透且不获焦点；同目标 metadata-only 观察保留流，切目标、取消、目标关闭清理。`sharing.png` / `pointer.png` 已实际打开核对。这个自窗口结果不冒充真实 WPS 或跨进程录屏验收。
- 异步 MCP 采样期间存在人工移动鼠标，记录中保留 `pointerUnchanged=false`，**不把该样本算作硬件隔离证明**。显示模块的同步调用保位检查通过；另外两个 MCP 双击/拖动样本记录物理位置和前台窗口均不变。不承诺输入法组合文本、用户同时编辑同一应用或所有第三方应用的完整焦点隔离。
- 验收先后复现并修正了局部坐标 Y 翻转（误点到 Drag Region）、滚轮缺少 NSEvent 真正窗口编号（事件未到目标），没有将初版失败算为通过。冷启动出现 `658×570 → 660×572` 几何过渡，旧引用正确返回 `stale_window`；测试只对只读枚举/观察有界刷新，绝不自动重放输入。
- 静态回归：Rust **44** 项、TypeScript **34** 项、Swift 无输入检查 **7** 组，Clippy `-D warnings`、fmt、四包构建/协议一致性/typecheck 通过。core 原超500行，测试机械移到独立文件后生产文件493行；Swift 按桥接、输入、会话显示分文件。没有新增运行时依赖或 wire 类型。
- 当前宿主录屏权限仍未授予；没有 reset TCC、打开设置或替用户点击开关。本轮跨进程整套截图/图像坐标、重新实现后的 stop/EOF/TTL 真实持键中断、release 和 Windows/Linux GUI **未重跑**，历史20组不能冒充本轮结果。构建已更新，不替用户终止现有 Claude Code/MCP 进程；重连后才载入新版本。

## 2026-10-22 需求纠正：只请求 macOS 系统权限

此前自行加入的 SDK 强制审批回调、MCP elicitation 表单、逐应用授权缓存、应用身份 CLI 和自制授权测试窗口均已撤回；它们不是用户要求的辅助功能/屏幕录制系统授权。保留既有 Agent → SDK/MCP → helper 与固定 policy，不要求 Agent 接入额外审批 UI。

SDK/MCP `start` 现在只做 macOS 系统权限预检 → 缺少时直接调用官方请求 API → 独立复查。是否授予由用户在 macOS 中决定；没有授予返回 `permissions_pending`，不启动控制会话。截图关闭的 policy 不请求屏幕录制；正常 GUI 回归预检仍静默，不重置 TCC、不自动点击系统权限开关。

真实系统请求已执行一次，结果保存在 `.local/m2-interactive-acceptance/system-request.json`，仍为 `Accessibility=true`、`Screen Recording=false`。不能据此声称系统弹窗已经显示或权限已经授予。误加的可见应用审批测试已退出且未通过完整验收；它的结果不算系统权限验收，产物仅保留在被忽略的 `.local/` 供追溯。

- 纠正后四包构建、协议一致性、typecheck 及 **33 项 TypeScript 测试**通过；**41 项 Rust 测试**、Clippy `-D warnings`、fmt、**6 组 Swift 无输入检查**通过。没有新增依赖或 wire 类型。
- `.local/system-permissions-only/live-XJeymz/results.json` 验证真实 SDK 不带审批回调即可启动，以及没有 elicitation 能力的普通 MCP 客户端也能启动。仅操作自建 fixture 元数据，禁止输入、未截图；请求截图权限的下一会话则正确返回 `permissions_pending`，未开始控制。
- 同次系统日志 `.local/system-permissions-only/tcc-correlated.log` 明确记录 `kTCCServiceScreenCapture, preflight=no`，证明发生了系统权限请求而非只做预检。本次父进程链的 TCC `responsible` 是 `/Applications/ChatGPT.app`，屏幕权限结果仍为未授予；不把这个宿主归属硬编码到其他 Agent，不推断未知的拒绝来源。完整授权后的截图/输入验收仍未通过。

## 2026-10-22 M0 执行内核、非 GUI 回归及 GUI 权限阻塞

本轮修复窗口实时校验、迟到窗口绑定，以及启动/激活的副作用不确定性。helper 使用可注入的后端和租约工厂，按会话、应用、窗口、动作、stdio、worker 拆分；Windows 后端按辅助功能、截图、物理输入和应用生命周期拆分。移动的 AX、截图及物理输入逻辑除接收者与模块可见性外保持原有实现，不增加模型能力或权限。

- 先完成行为保持的拆分，原有 **20** 项 TypeScript 测试仍通过；随后新增状态机回归，观察到 **7** 个失败场景后修复。继续通过红测复现并修复 worker 激活前缺少复验、激活不确定后仍可复用旧观察，以及 macOS bridge 丢失执行标记的问题。失败日志位于被忽略的 `.local/m0-baseline/`，没有把首轮失败记作通过。
- macOS 本机 `cargo test --workspace --locked` **41** 项通过：core 10、helper 29、macOS bridge 错误解码 2。新增用例覆盖关闭/移动/句柄或 PID 复用、pending/unknown 启动的重新发现、不因枚举扩大授权、投递后取消、旧观察失效、动作去重，以及 worker 错误边界。内存后端测试不是 GUI 或 OS 输入释放证据。
- TypeScript 四包构建、协议生成物一致性、严格类型检查及 **24** 项 Vitest 通过。新增测试证明 native 的不确定错误会转换为 `UnknownExecutionError`，明确的投递前拒绝不被误报为已执行；没有自动重放。
- Swift 无输入桥接检查 **6** 组通过，新增投递前/后错误标记检查。该检查不激活窗口、不截图、不投递输入，也不请求权限。
- macOS workspace build、fmt、Clippy `-D warnings` 通过。Windows `x86_64-pc-windows-msvc` workspace/all-targets 交叉类型检查和 Clippy `-D warnings` 通过；这不证明 Windows 链接、运行或 GUI 回归通过。
- 本轮 macOS debug GUI 预检 **blocked / exit 2**：`.local/macos-gui-UVZFEw/results.json`。helper 返回 `Accessibility=true`、`Screen Recording=false`，脚本在启动 fixture 前停止，没有截图、输入投递、权限请求或 TCC 修改。Windows GUI 未重跑；下方历史 GUI 证据不作为本轮修改后的验收。用户手动补齐负责宿主/helper 的屏幕录制权限后，仍须在隔离交互桌面重跑 SDK/MCP GUI、stop/EOF/TTL 与启动故障注入。

## 2026-10-19 macOS 本机实施与 GUI 验收通过

用户手动授予 Accessibility/Screen Recording 后，debug `.local/macos-gui-4LrtE5/`、release `.local/macos-gui-0vRDp9/` 各 **20 组真实 GUI 验收通过**。最终 helper `--permissions` 返回两项均为 true。两批 `after-input.png` 均已实际打开核对：Count 3、Hello 中文 👋、多行中文/换行/🌍、密码掩码及滚动后的内容；不是只检查 PNG 头。

通过内容：AX 树和密码脱敏、真实 ScreenCaptureKit、元素/图像坐标点击、Command+A 与单行/多行 Unicode 输入并重新观察、NSEvent 双击/三击计数、滚动、拖动、观察失效、去重/冲突、跨 helper 桌面租约、移动窗口拒绝旧引用、显式激活/物理输入保持几何、真实持鼠标拖动中的 stop/EOF/TTL 取消与释放、默认拒绝及截图/输入独立授权、官方 MCP client 工具/图片/点击/停止。fixture 均已关闭，会话已停止；没有控制个人应用或上传截图。

本机实测发现并修正：
- 未打包 AppKit 进程的 NSRunningApplication.launchDate 为 nil 导致漏枚举：改用 proc_pidinfo 的内核启动秒/微秒绑定进程身份。
- Swift JSON dictionary 整体转 `[String:Double]` 会失败：逐字段 NSNumber 解码，拒绝布尔、非有限或非正尺寸。
- 截图以两倍尺寸配置时默认不缩放，产生黑边：设置 scalesToFit 并排除窗口阴影，真实截图重新核验。
- Dock 有覆盖全屏但并不遮挡点击的透明系统窗口，矩形命中误拒绝：改用 AX 点命中并校验所属 PID 和目标 AX 窗口，不简单忽略系统窗口。
- 滚轮事件未指定 location，鼠标移动异步时投递到旧位置：显式设置事件位置；fixture 同时记录真实滚动事件与内容偏移。
- 曾误把 EOF 退出归因为缺少显式 break，并加了取消后立即退出，导致 act 被 stop 取消时 stop 响应丢失；真实 GUI 中失败后撤回该 break。保留排队响应处理，EOF 由 reader drop sender 后自然退出。新增无 GUI 的真实 helper queued-stop/EOF 回归，不把早期失败算为通过。
- 新增仅宿主 CLI 的 `--permissions` / `--request-permissions`：分别检查与显式请求系统官方提示；不暴露为模型工具、不绕过 TCC。

当前 Swift 无输入检查为 **5 组**（内核身份、JSON bounds、捕获预算、整数边界、多击元数据），Rust **13** 项、TypeScript **20** 项通过（含新增排队 stop/EOF 回归），fmt、clippy `-D warnings`、协议漂移和 typecheck 均通过。最终 debug 复验 `.local/macos-gui-Gmr40k/` 仍为 20 组通过。尚未覆盖真实个人/办公应用、所有显示器布局、非美式快捷键、强杀恢复或正式签名/公证分发。以下保留最初权限阻塞阶段的记录，不表示当前仍阻塞。

### 初始权限阻塞阶段

- 主机为 Apple Silicon / macOS 27.0.1，Rust 1.94.1、Swift 6.4、Node 24.14.0；使用锁文件安装项目依赖，未修改全局配置或系统授权。
- 当前基线提交 `85dd717` 的 GitHub Actions run [39145044201](https://github.com/clddup/computer-use/actions/runs/39145044201) 已通过 macOS 14 原生构建、Rust 测试及 TypeScript 检查；历史“macOS 从未编译”的描述已过时。这不代表交互桌面验收通过。
- 本机构建 debug/release helper 成功；`otool -L` 显示依赖 `@rpath/libcu_macos_bridge.dylib`，实际 capabilities 请求成功，证明开发树内 bridge 可加载，不证明移出构建树后的分发可用。
- 实施：CoreGraphics click-state 元数据、有界激活等待/几何复验、公共 `activate_window`、截图前像素预算/排除窗口阴影、截图超时取消任务、输入权限复查及桥接整数边界。helper 退出路径的早期修改已在后续真实 GUI 回归中撤回，原因见上。未实现 app discovery/launch 或 AX semantic，不把缺 `set_value` 当作缺文本输入。
- 新增 AppKit fixture 与 `scripts/macos-e2e.mjs`，状态由原生控件及独立 oracle 核对；测试产物仅在被忽略的 `.local/`。完整脚本覆盖 SDK/MCP、AX/截图/Unicode、多击、滚动拖动、去重、观察失效和 stop/EOF/TTL，但下述权限阻塞意味着这些 GUI 断言尚未实测通过。
- `pnpm --dir typescript test:macos:bridge` **3 组通过**：截图尺寸/像素预算、有限整数边界、多击 CGEvent 元数据；只构造事件，不投递、不截图、不触发权限请求。首轮测试编译因 CGFloat/Double 常量重载歧义失败，显式指定 CGFloat 后重跑通过，未把首轮记为成功。
- `test:gui:macos:smoke` **通过**：`.local/macos-gui-dNgph0/`，自建窗口真实启动并写入 oracle，随后关闭；仅启动验证，不是输入/截图验收。
- `test:gui:macos` **blocked / exit 2**：`.local/macos-gui-P9gNa1/`，helper 报 `Accessibility permission is missing`，脚本在启动 fixture 前停止，无输入投递。屏幕录制权限及真实 GUI 输入尚未验证，需用户手动授权负责的宿主/helper 后继续；没有绕过 TCC 或控制个人应用。
- Rust 基线 **13** 项、TypeScript **19** 项通过；新增无权限 Swift 检查已接入 macOS CI 步骤，本轮工作区改动尚未提交/推送，不能称新 CI 已运行。

## 2026-10-19 窗口激活与 WPS 实测

- 移除激活过程中的无条件 `SW_RESTORE` 及 `SwitchToThisWindow`；保持普通/最大化窗口状态，已最小化窗口仍按现有契约拒绝。物理输入在激活之后再次验证进程身份和窗口几何，拒绝使用已变化的旧坐标。
- Windows 枚举保留可见、非最小化、有有效 bounds 的无标题窗口；helper 的授权进程过滤不变，修复 WPS 独立新建弹层被漏掉的问题。
- 新增真实 fixture 回归：后台普通/最大化窗口的显式激活和物理键盘输入均不改变 bounds；无标题窗口可枚举和截图观察。debug `.local/windows-gui-gx6mm1/`、release `.local/windows-gui-Nf509E/` 均 **26** 组通过。
- release 首轮在读取 fixture 正在写入的 oracle 文件时读到空内容，脚本失败；加入有界读取重试后重跑通过，不计首轮为成功。
- Rust **13** 项、fmt、clippy `-D warnings`、debug/release build 通过；TypeScript 四包构建、协议漂移、typecheck、**19** 项测试通过。冷启动 debug `.local/windows-cold-start-hMNR46/`、release `.local/windows-cold-start-pg8yK2/` 通过。
- 用户授权的 WPS 实测经本项目 SDK/helper 启动和操作：观察无标题新建弹层，UIA Invoke“文字”，图像坐标点击“新建空白”，物理 Unicode 输入标题和四段示例自我介绍；截图核实正文，WPS 状态栏字数 362。新文档保持未保存，会话已停止。此演示先于激活尺寸修复，不冒充修复后 WPS 回归；修复后通过的是自建 fixture。未读取已有文档正文、未保存/上传，个人应用截图仅留本地，不随源码发布。

## 2026-10-18 Windows semantic 增量验收

- Rust **13** 项测试（core 10、helper 3）、Windows workspace clippy `-D warnings`、debug/release build 通过；Linux aarch64 cross-check 通过。
- TypeScript tsdown/Rolldown、协议漂移、no-emit typecheck、**19** 项 Vitest 通过，新增版本协商和 semantic wire 样例。
- GUI debug `.local/windows-gui-G0mxd4/`、release `.local/windows-gui-suYOge/`：保留原 16 组，另验证 UIA invoke/toggle/select/set_value、无 pattern 拒绝及显式物理 click、后台 provider 行为、UIA 响应丢失、旧 v1 profile；共 **24** 个通过组，results 另含一条 foreground 观测记录。
- 冷启动 debug `.local/windows-cold-start-A3hYzA/`、release `.local/windows-cold-start-qMU4iq/` 通过；目标仍由 helper 启动，不以 host 预启动冒充。
- 四种 UIA action 均验证状态变化、真实 mechanism、鼠标坐标不变、无鼠标消息、action_id 去重；SetValue 验证无键盘消息。官方 MCP client 也执行真实 UIA invoke。
- fixture 增加 cursor physical coordinates/visibility、foreground HWND/PID、active control、鼠标/键盘消息累计计数，短点击不只靠 100ms mouse_down 快照。结果通道不执行生产输入；新增 background 控制仅创建测试前台 sentinel。
- **实测限制**：后台 WinForms Invoke 会由 UIA/provider 改变 foreground；首次要求“前台必定不变”的测试失败，保留为限制并改为记录 provider 行为，未通过伪造结果掩盖。cursor 不动不等于用户输入完全隔离。
- debug 初始及输入后 PNG 已打开查看，显示 Count 2、Hello 中文、滚动变化和密码掩码；WGC 不画 cursor 不等于系统 cursor 被隐藏。
- **仍待补验**：真实阻塞 UIA provider 的 stop/EOF/TTL/12 秒超时故障注入。当前 worker 取消路径已实现，旧物理输入 stop/EOF/TTL 与 UIA 响应丢失通过，但不能据此冒充所有 semantic 阻塞场景实测完成。焦点/选区/文档 production observation 仍未实现。
- 本次未运行 Codex 私有 helper、未新增 cursor hide/虚拟 desktop、未修改全局配置、未提交/推送。下面表格为此前基线记录。

## 环境

- Windows 11 Pro build 26200，交互桌面，fixture 截图对应 125% 缩放。
- Rust/Cargo 1.90.0；Node 24.14.0；pnpm 11.13.0。
- Node 全局默认通过用户已授权的 mise 切换为 24.14.0；原配置为 `latest`。未修改全局 MCP/Agent 设置。
- Linux check 使用已安装 `aarch64-unknown-linux-gnu` target，不是本机 Linux linker 或桌面测试。

## 已通过

| 检查 | 实际结果 |
| --- | --- |
| Windows `cargo build --workspace --locked` | 成功，真实 Win32/UIA/WGC/SendInput 代码编译 |
| Windows `cargo build --release --locked` | 成功，优化版 helper 也通过完整 16 组真实 GUI 验收及冷启动验收 |
| `cargo test --workspace --locked` | **12** 项通过：core 9、helper 3；其他 crate 无独立单测，不冒充全覆盖 |
| Windows `cargo clippy --workspace --all-targets --locked -- -D warnings` | 成功，无警告 |
| `pnpm --dir typescript check` | tsdown/Rolldown 构建四包、schema/types 漂移检查、严格 no-emit typecheck、**17** 项 Vitest 均通过 |
| Linux `cargo check --workspace --target aarch64-unknown-linux-gnu --locked` | 成功，只做类型检查，不证明链接/运行 |
| Linux target `cargo clippy --workspace --all-targets --target aarch64-unknown-linux-gnu --locked -- -D warnings` | 修正 scanline stride 的 div_ceil lint 后通过；新增异常 padding 拒绝 |
| 文档内部链接检查 | README、docs、skill 共 8 个 Markdown 文件的本地链接均可解析 |
| Windows `node scripts/windows-e2e.mjs` | **16** 组真实 fixture 验证通过，含 SDK 和官方 MCP Client |
| Windows `node scripts/windows-cold-start-e2e.mjs` | **通过**：helper 从固定 host-approved `allowed_apps` profile 启动自建 fixture（host 未预先 spawn），完成 list/launch/get/activate/observe/click/stop；debug 证据在 `.local/windows-cold-start-2TlYzh/`，release 证据在 `.local/windows-cold-start-PRlbj6/` |

### Windows GUI 最后完整通过批次

Debug 通过批次：`.local/windows-gui-WSD5BJ/results.json`；release 通过批次：`.local/windows-gui-9pZo6u/results.json`。冷启动 debug/release 证据分别见上表。每批初始真实 WGC 图片为 `window.png`，输入/滚动后的图片为 `after-input.png`。Debug 初始及输入后、release 输入后的截图已实际打开查看，不仅检查 PNG 头；后图显示 `Hello 中文`、Count 2、滚动条变化和掩码密码框。证据被 gitignore，不随源码发布。

测试项目：

1. UIA 树、已标记密码节点不泄露固定假密码、真实 WGC PNG。
2. 元素点击改变 oracle；相同 action_id 不重复点击，参数冲突拒绝，action_status 返回原记录。
3. 按图像像素换算点击，按钮计数再次增加。
4. 聚焦、Ctrl+A、中英文输入及重新观察 value。
5. 已消耗/被新观察替换的观察，以及不属于观察的元素引用拒绝。
6. 滚动真实改变自建 panel 的 AutoScrollPosition。
7. 未中断拖动完成，鼠标/修饰键未残留。
8. 第二个独立 helper 无法取得控制租约。
9. fixture 通过专用测试控制文件移动后，旧观察动作在投递前拒绝。
10. 观察到**真实 mouse-down**之后 stop；动作 unknown，键鼠释放。
11. 停止后另一个 helper 可以取得租约。
12. 真实 stdin EOF 中断已按住鼠标的拖动，释放键鼠。
13. 3 秒会话 TTL 在拖动尚未完成时独立取消，结果 unknown 并释放。
14. 测试专用 C# stdio proxy 丢弃真实点击响应：按钮确实只加一次，SDK 超时 UnknownExecutionError，不自动重发；仍活动的会话可以查询 dispatched 日志。
15. 无策略默认拒绝；截图与输入独立授权校验，拒绝输入不改变计数。
16. 官方 MCP Client 真实 stdio 工具发现、拒绝自授权参数、图片内容、点击复选框及 stop。

所有自动输入只针对项目自建测试窗口，未控制个人应用。fixture oracle 是结果核对通道，只有移动/关闭等测试控制，不被生产 runtime 当作输入执行捷径。

### 单测说明

TypeScript transport/runtime 测试中使用模拟子进程/传输，验证 chunk 分帧、超限、断线、超时不重放、非法响应、队列限制、busy/stop 和结果异常的 unknown 语义。**这些 mock 测试不代替上面的真实 GUI、EOF 与响应丢失测试**。

协议测试实际启动 Rust helper `--validate-protocol`，核对合法/非法样例与 Ajv 一致，检查超限/半行 EOF。version 的运行时协商核验与类型解码分开；u64 超出 JS 安全整数的范围不在支持的互操作值域内。

## 实测发现并修正的问题

- Windows pnpm workspace junction 在本机 Node stat/realpath 失败：hoisted + injected、关闭 injected 去重解决；不是通过提权或修改系统权限绕过。
- Rust flatten 请求与生成 schema 的 additionalProperties 冲突：改为嵌套 command，重新生成并做双语言样例验证。
- Ajv 子类型校验误继承聚合 root required：只携带 definitions/$ref 编译。
- 移动测试混用 WinForms 逻辑坐标与 DWM 物理坐标：oracle 比较自身前后逻辑位置，runtime 继续核验原生物理几何。
- 响应丢失测试 proxy 最初使用系统代码页，导致中文 JSON 损坏：测试 proxy 的 console/子进程流显式 UTF-8；修正后真实丢包验证通过。
- TTL 原先仅在调度循环空闲时检查：增加独立单调时钟 watchdog，并做真实持鼠标拖动到期测试。
- act result 结构校验异常原先可能退化为普通 Error：现在保守抛 UnknownExecutionError，且有回归测试。
- Linux key-release 循环不再在可能失败的 drain 迭代中丢失剩余 held keys；macOS Unicode 路径补充 held key 清理记录。这些平台修正尚未有各自 GUI 证明。

## 尚未验证 / 尚未完成

- **Codex/WPS 类真实应用**：应用 profile 已支持 Windows 冷启动，但尚未用 WPS 或个人应用做 GUI 验收；不得把自建 fixture 结果当作 WPS 已成功。用户测试前仍需宿主明确写入 WPS 的固定、可审查启动 profile，并使用新空白文档。
- **macOS**：Apple Silicon 本机 debug/release 各 20 组 SDK/MCP fixture GUI 验收通过；仍需验证真实应用、所有显示器布局、不同宿主的 TCC 归属，以及签名/公证和离开构建树后的分发加载。
- **Linux**：尚未原生链接/GUI 测试；EWMH frame、AT-SPI toolkit 兼容性与 Unicode 编辑语义待验。输入路径的同步 X11 roundtrip 在 X server 挂死时可能延迟取消，不保证该故障下有界停止。
- **CI**：GitHub Actions run [39046692426](https://github.com/clddup/computer-use/actions/runs/39046692426) 已通过 Windows、Linux/macOS 原生构建、Rust 测试和 TypeScript 检查；workflow 已迁移到 Node 24-compatible actions，GitHub hosted runner 结果不替代 macOS/Linux 原生 GUI 验收。未发布 npm 或安装包。
- 观察树/截图非原子、locator 非强控件身份，TOCTOU 无法完全消除；Unix lease symlink 检查不是完整抗同用户竞态沙箱。
- helper 崩溃/强杀/系统故障时不保证输入释放；不提供跨 epoch exactly-once，不把 dispatched 报为 verified。
- 尚未形成三平台安装包/自动下载分发渠道，未发布 npm；当前是源码构建集成。

因此交付状态是：**Windows 可接入且有真实验收证据的 v0.1；三平台源码均有真实实现，但整体三平台验收未完成。**
