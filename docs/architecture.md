# 架构与协议 v1/v2

## 工程边界

```text
Agent
  ├── TypeScript SDK ──┐
  └── MCP stdio ───────┤
                  ComputerRuntime
                       │ 私有、有界 NDJSON stdin/stdout
                    cu-helper
                       ├── Windows: Win32 + UIA + WGC + SendInput
                       ├── macOS: AX + ScreenCaptureKit + CoreGraphics (Swift bridge)
                       └── Linux: X11/XTEST + AT-SPI D-Bus
```

Rust 工程位于 `native/`，Node 工程位于 `typescript/`。MCP 与 SDK 不各自实现输入语义，共用 runtime；原生 helper 再执行权限、窗口身份、观察有效期和动作去重核验。TS 不加载 Rust 源码或私有 SDK。

## Wire

请求为 `{version:1,id,command:{method,params}}`，响应为 `{version:1,id,result,error}`，正常响应只有 result 非 null，错误响应只有 error 非 null。每个 JSON 后必须换行；单帧上限 **16 MiB（包括换行）**。不接受未知结构字段。`id` 是响应关联 ID，不是通用请求去重键；只有 `action_id` 有动作日志去重语义。

SDK/runtime 首先使用 v1 `capabilities` 探测最高 `protocol_version`，支持 v2 时后续使用 v2；响应 envelope 回显请求版本，transport 按每个 pending request 校验。v1 observation 省略 element capabilities，v1 action 列表不宣告新增 Windows 语义动作；旧 JSON 形状保留。v2 增加元素 `capabilities` 和 `invoke`、`toggle`、`select`；Windows `set_value` 也要求 v2。`VERSION=1` 是最低兼容 profile，`MAX_VERSION=2` 是本 helper 上限。

Rust `cu-protocol` 是 wire 类型源，schemars 生成 JSON Schema，再生成 TS 类型。`scripts/generate-protocol.mjs --check` 检测漂移。Ajv 显式验证有限浮点数、int32、uint32 与 JS 安全非负 uint64。Rust 的 u64 解码范围比 JS 安全整数大；时间戳的受支持跨语言范围限定为 `0..Number.MAX_SAFE_INTEGER`，不把超过此范围的数据视为互操作值。

方法：capabilities/start/list_apps/launch_app/list_windows/get_window/activate_window/observe/act/action_status/stop。`allowed_apps` 是宿主策略中的固定启动 profile，不能由工具参数临时添加；`launch_app` 只执行 profile，返回 `pending` 不等于已有可操作窗口。策略不是工具参数，由宿主以绝对文件路径传给 helper。协议并非对不可信同用户进程的安全 RPC 边界；能任意启动原生程序或改写宿主文件的进程不受此策略沙箱化。

## 会话与观察

- 会话可以从固定 `allowed_apps` profile 冷启动；启动后以真实可见窗口重新建立进程身份绑定。`get_window`/`activate_window` 重新核验窗口引用，激活后应重新 observe。
- 每个 helper 仅允许一次 start；stop 后创建新 runtime/helper。
- 会话固定批准 PID 或 host-approved application profile 的进程启动身份，窗口引用包含 PID、启动身份、原生窗口 ID 和几何。
- 每用户一个 OS 文件锁：保守覆盖所有显示器/桌面。锁直到执行线程退出输入路径、释放 RAII guard 后才释放，不允许按 TTL 抢锁。
- 单一当前观察，默认有效 30 秒；observe 清旧观察，进入可能副作用的动作前消耗观察。默认请求截图、关闭 accessibility；调用方需要结构化树/焦点信息时显式请求 accessibility。
- 元素和图片 ID 必须属于当前观察；图片像素通过 `screen_bounds` 映射到桌面，支持缩放及负坐标，最终点必须在授权窗口内。
- 每动作重核验窗口身份和几何；元素目标重新读取并匹配 locator、role、name、bounds、password、value。locator 是路径，不是强内容身份；同路径相同属性的控件替换可能无法识别。
- UIA/AX/AT-SPI 与截图不是原子快照；拒绝检测到的变化，不声称完全消除 TOCTOU。

## 动作与不确定性

旧 `click`（支持有界 `click_count`）、`type_text`、`key`、`scroll`、`drag` 仍保留物理契约。v2 Windows 提供独立的 `invoke`、`toggle`、`select` 和 `set_value`，只接受当前 observation 内的 element_id，按真实 UIA pattern 声明能力；分别返回 `uia-invoke`、`uia-toggle`、`uia-selection`、`uia-value-pattern`。Toggle 是一次状态循环，Select 是 SelectionItem.Select，SetValue 是整值替换，不是 literal type_text。Linux set_value 仍使用 AT-SPI；macOS 不宣告 set_value；新增 UIA 动作不冒充跨平台实现。

语义路径不调用 SetCursorPos、SendInput 或显式前台激活。应用/UIA provider 仍可能自己改变焦点或前台；WinForms 后台 Invoke 已实测发生前台变化，因此不承诺后台输入隔离。无 pattern 时投递前 rejected；调用方需要重新 observe、用新 action_id 显式物理 click，mechanism 为 `sendinput`。绝不在不确定 UIA 调用后自动 fallback。

- **rejected**：输入投递前的校验拒绝。
- **dispatched**：OS/API 接受；不代表应用已完成目标。
- **verified**：协议保留状态，v0.1 后端不会自行返回此状态。
- **unknown**：可能部分投递、取消、断线或响应遗失；不得自动重放。

输入开始前日志写 provisional unknown；相同 action_id、相同观察 ID 和参数返回原记录，不再次投递；参数冲突拒绝。每 epoch 日志最多 1024 项。重启不继承日志、不承诺跨崩溃 exactly-once。状态查询只在活动会话仍保留时可用，stop/EOF/expiry 后日志随会话释放。

## 停止路径

stdin reader 与动作执行解耦，stop/EOF 和 CTRL-C 原子设置取消标记；独立单调时钟 watchdog 在会话 TTL 到期时设置标记。阻塞 UI 查询在独立 helper `--probe` 子进程中，父进程可取消或在 12 秒 deadline 后终止；prepare worker 可以激活/raise 窗口，但不执行键鼠输入。它不是 OS 沙箱。

物理键鼠输入在父 helper 执行线程，逐批/拖动约每 10ms 检查取消。UIA semantic action 先在只读 worker 做 live locator/pattern 预检，成功后写 provisional unknown，再在独立 `--probe` worker 调用 pattern；父进程在取消或 12 秒 deadline 后终止 worker，之后只报告 unknown，不重放。终止 worker 不能撤回 provider 已接受的动作。RAII/defer 释放本项目按下的键鼠。stop 不能撤销已经被应用收到的输入，强杀/进程崩溃也不保证释放；TS 等待优雅退出，超过 6 秒终止后报告 cleanup_unconfirmed，不伪称清理成功。
