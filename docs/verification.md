# 实际验证记录

日期：2026-10-17。此处区分源码实现、类型检查、单测和真实桌面测试，不把 CI 配置或 cfg 排除路径当作平台验证。

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
