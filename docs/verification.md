# 实际验证记录

日期：2026-10-19（含 2026-10-17/18 基线）。此处区分源码实现、类型检查、单测和真实桌面测试，不把 CI 配置或 cfg 排除路径当作平台验证。

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
- **macOS**：Swift bridge 尚未在 Mac 上编译/加载；AX、TCC、ScreenCaptureKit、坐标和停止 GUI 验收、签名/公证仍待真实环境。rpath 元数据已接到最终 helper，但当前不声称已验证正确加载。
- **Linux**：尚未原生链接/GUI 测试；EWMH frame、AT-SPI toolkit 兼容性与 Unicode 编辑语义待验。输入路径的同步 X11 roundtrip 在 X server 挂死时可能延迟取消，不保证该故障下有界停止。
- **CI**：GitHub Actions run [39046692426](https://github.com/clddup/computer-use/actions/runs/39046692426) 已通过 Windows、Linux/macOS 原生构建、Rust 测试和 TypeScript 检查；workflow 已迁移到 Node 24-compatible actions，GitHub hosted runner 结果不替代 macOS/Linux 原生 GUI 验收。未发布 npm 或安装包。
- 观察树/截图非原子、locator 非强控件身份，TOCTOU 无法完全消除；Unix lease symlink 检查不是完整抗同用户竞态沙箱。
- helper 崩溃/强杀/系统故障时不保证输入释放；不提供跨 epoch exactly-once，不把 dispatched 报为 verified。
- 尚未形成三平台安装包/自动下载分发渠道，未发布 npm；当前是源码构建集成。

因此交付状态是：**Windows 可接入且有真实验收证据的 v0.1；三平台源码均有真实实现，但整体三平台验收未完成。**
