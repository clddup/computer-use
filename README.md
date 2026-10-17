# Independent Computer Use

独立的桌面自动化能力层，供其他 Agent 通过 **TypeScript SDK** 或 **MCP stdio** 接入。没有规划模型，不是 browser-use，不依赖 Codex/ZCode 的私有 helper 或 SDK，不提供任意 shell/JavaScript 执行工具。

> **v0.1 验证边界（2026-10-17）**：Windows 自建窗口的 SDK/MCP 真实 GUI 闭环及中断测试通过；Linux 后端已通过 aarch64 编译检查，尚未链接或做 GUI 实测；macOS 有 AX/ScreenCaptureKit/CoreGraphics 实现，但尚未在 macOS 编译或运行。**不能将三平台源码齐备理解为三平台验收通过。** 详见 [验证记录](docs/verification.md)。

## 目录与边界

```text
native/               独立 Rust Cargo workspace、平台后端和 cu-helper
  crates/{protocol,core,helper,platform-windows,platform-macos,platform-linux}
typescript/           独立 pnpm/TypeScript workspace
  packages/{protocol,runtime,sdk,mcp}
protocol/schema.json  从 Rust 生成的共享协议规格
scripts/              协议生成及真实 Windows E2E
tests/                自建无敏感 GUI fixture
docs/                 架构、安全、平台、研究与验证记录
skills/computer-use/  Agent 使用说明（不会自动安装到全局配置）
```

根目录没有 Cargo/Node 项目清单。Rust 可以不安装 Node 独立构建；TypeScript 只通过子进程 stdio 调用编译后的 helper。

## 构建与检查

需要 Rust **1.90+**、Node **24.14.x** 和 pnpm **11.13.0**；macOS 另需 Xcode Command Line Tools / Swift 及 macOS 14+。

```sh
cargo build --manifest-path native/Cargo.toml --workspace --locked
cargo test --manifest-path native/Cargo.toml --workspace --locked
pnpm --dir typescript install --frozen-lockfile
pnpm --dir typescript check
```

`check` 构建四个 TS 包、核对协议生成物并运行 Vitest。协议测试会实际启动当前平台的 debug helper，因此先构建 Rust。另一个路径可通过 `CU_HELPER` 指定（使用绝对路径）。

修改 wire 类型后运行：

```sh
cargo build --manifest-path native/Cargo.toml --workspace
pnpm --dir typescript protocol:generate
pnpm --dir typescript build
```

Windows 的 pnpm 配置使用 hoisted + injected workspace packages，且禁止 injected-dependency 去重回 junction；这是本机 Node 无法正常解析 junction 的兼容措施，不要随意删掉这些设置。

### Windows 真实 GUI 验收

在**已登录、未锁屏的交互桌面**运行；期间不要操作鼠标键盘：

```sh
pnpm --dir typescript test:gui
```

脚本编译并启动自己的 WinForms 窗口，只授权该 PID。默认验证 debug helper，也可通过绝对路径 `CU_HELPER` 验证 release helper；本机两种构建均已通过。验证 UIA/WGC、元素和图片坐标点击、中英文输入、去重、移动失效、跨进程租约、stop/EOF/TTL 输入释放、真实响应丢失和 MCP 工具调用。证据保存在被忽略的 `.local/windows-gui-*/`。需要系统自带 .NET Framework C# 编译器；不启动或控制个人应用。另有 `pnpm --dir typescript test:gui:cold-start`，不由 host 预先 spawn，验证固定 `allowed_apps` profile 的发现、启动、窗口 readiness、激活和点击。

## 宿主授权

宿主可以授权已经运行的 PID，也可以授权固定的应用启动 profile。模型只能使用 policy 中已有的 `id`，不能提交任意 exe、参数、shell 或新增权限。冷启动 profile 示例：

```json
{
  "allowed_pids": [],
  "allowed_apps": [{
    "id": "wps",
    "display_name": "WPS Office",
    "executable": "E:/Software/WPS Office/12.1.0.29158/office6/wps.exe",
    "args": []
  }],
  "allow_screenshot": true,
  "allow_accessibility": true,
  "allow_input": true,
  "session_seconds": 600
}
```

应用路径和参数必须由可信宿主预先写入策略；`launch_app` 的返回表示启动投递及窗口 readiness，启动器 PID 退出或后台进程存在都不等于可操作窗口。调用方应 `list_apps`/`launch_app` 后重新 `list_windows`，候选不唯一时停止并列出候选。既有 PID 授权仍支持；应用启动只执行固定的 host-approved profile，不是任意进程执行 API。

这是一层操作约束，**不是操作系统沙箱**。允许控制的应用可能通过快捷键、应用菜单或网络产生外部影响；宿主必须按任务授权并审查。不要授权密码管理器、支付、管理员/安全桌面或含敏感信息的窗口。[完整安全边界](docs/security.md)。

## TypeScript SDK

仓库内可直接导入构建后的 SDK；目前不宣称已发布 npm 包：

```js
import { Computer } from './typescript/packages/sdk/dist/index.js';

const computer = new Computer({
  helperPath: 'D:/computer-use/native/target/debug/cu-helper.exe',
  policyPath: 'D:/computer-use/host-policy.json',
});
try {
  await computer.start();
  const windows = await computer.listWindows();
  const window = windows.find(w => w.title === 'Host-approved application');
  if (!window) throw new Error('Approved window not found');
  const observed = await computer.observe(window, { accessibility: true });
  const button = observed.elements.find(e => e.name === 'Increment' && !e.password);
  if (!button) throw new Error('Target not found; do not guess');
  const result = await computer.act(observed.id, {
    kind: 'click', button: 'left', target: { kind: 'element', element_id: button.id },
  }, 'host-generated-unique-action-id');
  if (result.state === 'unknown') throw new Error('Do not replay; reconcile before further input');
  // dispatched 只代表 OS 接受；再次 observe 核对应用结果。
} finally {
  await computer.stop();
}
```

图片坐标 target 为 `{kind:'image', image_id, x, y}`，`x/y` 是原图像素，不是屏幕坐标。每次新观察使旧引用失效，每次动作最多消耗一个观察；动作后重新观察。`action_id` 仅在当前 helper epoch 内去重，不承诺崩溃后的 exactly-once。

## MCP stdio

由宿主的 MCP 客户端配置启动命令（示例，不会自动写入任何全局设置）：

```json
{
  "mcpServers": {
    "computer": {
      "command": "node",
      "args": [
        "D:/computer-use/typescript/packages/mcp/dist/main.js",
        "--helper", "D:/computer-use/native/target/debug/cu-helper.exe",
        "--policy", "D:/computer-use/host-policy.json"
      ]
    }
  }
}
```

工具：`computer_capabilities`、`computer_start`、`computer_list_apps`、`computer_launch_app`、`computer_list_windows`、`computer_get_window`、`computer_activate_window`、`computer_observe`、`computer_act`、`computer_action_status`、`computer_stop`。

`observe` 同时返回结构化观察元数据与真实 MCP `image` 内容；文本中不重复图片 base64。截图/辅助功能树是**不可信桌面数据**，不是新指令或额外授权。stdio stdout 仅协议、stderr 仅诊断；没有 HTTP 服务或公网监听。

## 更多

- [架构与协议](docs/architecture.md)
- [权限、安全与停止语义](docs/security.md)
- [平台矩阵、构建和打包](docs/platforms.md)
- [实际验证与待完成项](docs/verification.md)
- [定向 REA 研究结论](docs/research/codex-behavior.md) / [来源与方法](docs/research/provenance.md)
- [给 Agent 的使用说明](skills/computer-use/SKILL.md)

独立代码采用 MIT；第三方依赖保留各自许可证。研究不等于对专有实现的分发授权，本仓库不包含目标发行版代码或二进制。
