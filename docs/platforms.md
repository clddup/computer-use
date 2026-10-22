# 平台与打包

| 平台 | 真实实现 | 本轮验证 | 限制 |
| --- | --- | --- | --- |
| Windows | Win32/DWM、UI Automation、WGC、SendInput；固定 host-approved app profile 的直接启动与窗口 readiness | Windows 11 build 26200 编译、单测、SDK/MCP 自建 GUI 实测及 debug/release 冷启动实测 | 可见普通完整性窗口；v2 UIA Invoke/Toggle/Select/SetValue 实测，不控制 UAC/安全桌面 |
| macOS | AX、ScreenCaptureKit 单帧、CoreGraphics，Swift C ABI bridge；显式窗口激活 | Apple Silicon / macOS 27.0.1 debug/release 各 **20 组真实 SDK/MCP GUI 验收通过**，另有无输入桥接检查 | macOS 14+；需 AX/屏幕录制权限，窗口 AX 几何关联必须唯一；未覆盖所有应用/多屏组合或正式签名分发 |
| Linux | X11/EWMH、XTEST、AT-SPI D-Bus、root screen-region PNG | aarch64-unknown-linux-gnu cargo check 通过，**未链接/GUI 实测** | 仅 X11；无 native Wayland；截图可能含遮挡物，Unicode 使用 EditableText |

capabilities 报后端声明，不是所有权限、控件及截图都已验证可用的证明；具体失败由观察/动作返回。

Windows 目前是唯一实现并验收了 app discovery/launch 生命周期的平台；macOS/Linux 的应用发现/启动会明确返回 `unsupported_action`，不因工具被发现就宣称已具备跨平台启动。macOS 的 `activate_window` 已接通有界激活准备路径，并通过同进程双窗口的激活/几何保持验收。

## Windows

安装 Rust MSVC 工具链及 Visual Studio C++ Build Tools；构建 `native/Cargo.toml`。使用 Per-Monitor V2 DPI、进程创建时间身份、DWM frame 几何。WGC 使用真实窗口捕获，不暗中回退 GDI。实际帧尺寸与窗口几何不匹配时拒绝，避免错误坐标注入。

物理输入需要目标前台窗口；遮挡检查与动作前重验证仍不能消除所有异步窗口切换竞态。v2 UIA semantic action 不注入鼠标/键盘、不显式激活窗口，但 provider 可以改变前台或焦点；后台 WinForms Invoke 已实测抢前台，不能称为虚拟鼠标或输入隔离。UIA 调用在可终止子进程内。WGC `WithoutCursor` 仅排除截图光标，不隐藏系统 cursor。自建 fixture 的 125% 缩放截图与坐标点击已在本机实测，不代表所有 DPI/多屏组合已覆盖。

发布构建：`cargo build --manifest-path native/Cargo.toml --release --locked`。helper 在 `native/target/release/cu-helper.exe`；TS 使用 `--helper` 显式绝对路径。

## macOS

在原生 macOS 14+ 主机安装 Rust 和 Xcode Command Line Tools，执行同一 Cargo 命令。build.rs 调用 `xcrun swiftc` 构建 `libcu_macos_bridge.dylib`，不支持从 Windows 交叉构建 Swift bridge。helper 的 build.rs 将平台 crate 提供的 OUT_DIR 加入最终二进制 rpath，同时加入 `@executable_path`。

开发树依赖 OUT_DIR，分发时必须将 bridge dylib 放在 helper 旁边。可用命令定位和核对（只在 macOS）：

```sh
find native/target/release/build -name libcu_macos_bridge.dylib
# 复制本次构建对应的 dylib 到 helper 同目录后：
otool -L native/target/release/cu-helper
```

SDK/MCP 的 `start` 在 macOS 上先检查辅助功能/屏幕录制权限，缺少时调用 `AXIsProcessTrustedWithOptions` / `CGRequestScreenCaptureAccess` 请求系统官方授权，再独立复查。不增加自制应用授权框或审批回调；系统是否显示提示、用户是否授予由 macOS 决定。未授予返回 `permissions_pending`，需用户在系统提示或 System Settings → Privacy & Security 中完成授权。`cu-helper --permissions` 仅检查，`--request-permissions` 显式请求；可追加 `--policy`，截图关闭时不请求屏幕录制。不同签名/路径/TCC 归属可能影响负责的宿主/helper，当前不重置 TCC、不自动修改权限。GUI 测试预检仍静默，`capabilities.available` 只反映 AX 权限；正式签名/公证分发仍未验收。

本机验证命令（先构建 helper 和 TypeScript）：

```sh
cargo build --manifest-path native/Cargo.toml --workspace --locked
pnpm --dir typescript install --frozen-lockfile
pnpm --dir typescript check
pnpm --dir typescript test:macos:bridge
pnpm --dir typescript test:gui:macos:smoke
# 首次使用可显式请求官方授权提示，需用户手动允许：
native/target/debug/cu-helper --request-permissions
native/target/debug/cu-helper --permissions
pnpm --dir typescript test:gui:macos
```

- `test:macos:bridge`：编译真实 bridge，检查内核进程身份、混合 JSON 数值 bounds、截图尺寸预算、整数边界及 CGEvent 多击元数据；不截图、不投递事件、不请求权限。
- `test:gui:macos:smoke`：仅启动自建 AppKit 窗口并读取其 synthetic oracle；不证明截图/输入可用。
- `test:gui:macos`：仅操作新建 fixture，验证 SDK/MCP、截图/AX/输入、观察失效、去重及 stop/EOF/TTL。AX 或屏幕录制预检缺权限时在启动窗口前以 exit 2 阻塞；观察阶段权限被撤销也记为 blocked，不冒充通过。需要手动授权真实负责的宿主/helper，必要时重启宿主后重跑；测试脚本不修改系统权限。
- 每次测试产物位于被忽略的 `.local/macos-gui-*`，`results.json` 区分 `passed`、`blocked`、`failed`、`smoke-only`。测试期间暂停人工键鼠操作；真实截图通过后仍需打开核对，不仅检查 PNG 文件头。
- fixture 的 `command.json` 仅允许移动测试窗口与切换其内部窗口，用于故障注入；实际点击、文本等输入只能经 SDK/helper 发生。

## Linux

运行环境需要 X11 display、EWMH window manager、XTEST extension、用户 session D-Bus、AT-SPI registry 和启用辅助功能的 toolkit 应用。x11rb/zbus 使用 Rust 协议客户端，不要求开发机安装 Xlib headers。Unicode 文本通过当前批准 PID 内 focused EditableText.InsertText；set_value 走 AT-SPI SetTextContents，不能写不支持这些接口的控件。

AT-SPI 窗口关联依据 bus owner PID、标题与几何，模糊则拒绝。不改全局键盘映射；键盘注入仅支持当前布局 base-level 可表示的受支持键。对 reparenting WM 的遮挡检查沿 client parent 链查找顶层 frame，避免将正常 WM frame 当成另一应用；此路径和 AT-SPI 单窗口匹配仍未通过真实 Linux 桌面验证。输入期间同步 X11 roundtrip 依赖 X server 响应，当前不能保证 X server 挂死时的有界停止。

本机 WSL2 Ubuntu 24.04 有 DISPLAY 和 WAYLAND_DISPLAY，但没有已确认 Rust/GCC/Xvfb 等工具链，未安装系统包、未提权。WSLg/XWayland 也不能证明原生 Wayland 可控。

交叉 `cargo check --target aarch64-unknown-linux-gnu` 只做类型检查，不需要 Linux linker；这不验证可运行二进制。原生 Linux 上仍需 `cargo build/test` 以及隔离无个人数据桌面的真实 GUI 测试。

## TS 分发

四个 workspace 包目前 private/workspace 方式使用，未发布 npm。构建顺序 protocol→runtime→sdk/mcp，由 tsdown（Rolldown）生成 ESM 与声明文件，严格类型检查单独通过 `pnpm --dir typescript typecheck` 执行。分发 helper 是宿主责任，MCP 默认路径仅适合源码仓库 debug 布局，生产必须显式 `--helper`。不要把仓库的默认 debug 路径当作全局安装协议。
