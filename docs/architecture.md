# 架构与协议 v1

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

`click`（支持有界 `click_count`）、`type_text`、`key`、`scroll`、`drag`、`set_value` 是 wire 操作全集；应用生命周期方法不等同任意程序执行权限，可用集合以当前 backend capabilities 为准。Windows/macOS 不宣告 set_value；Linux type_text 使用 AT-SPI EditableText，报告实际机制。

- **rejected**：输入投递前的校验拒绝。
- **dispatched**：OS/API 接受；不代表应用已完成目标。
- **verified**：协议保留状态，v0.1 后端不会自行返回此状态。
- **unknown**：可能部分投递、取消、断线或响应遗失；不得自动重放。

输入开始前日志写 provisional unknown；相同 action_id、相同观察 ID 和参数返回原记录，不再次投递；参数冲突拒绝。每 epoch 日志最多 1024 项。重启不继承日志、不承诺跨崩溃 exactly-once。状态查询只在活动会话仍保留时可用，stop/EOF/expiry 后日志随会话释放。

## 停止路径

stdin reader 与动作执行解耦，stop/EOF 和 CTRL-C 原子设置取消标记；独立单调时钟 watchdog 在会话 TTL 到期时设置标记。阻塞 UI 查询在独立 helper `--probe` 子进程中，父进程可取消或在 12 秒 deadline 后终止；prepare worker 可以激活/raise 窗口，但不执行键鼠输入。它不是 OS 沙箱。

实际输入在父 helper 执行线程，逐批/拖动约每 10ms 检查取消。RAII/defer 释放本项目按下的键鼠。stop 不能撤销已经被应用收到的输入，强杀/进程崩溃也不保证释放；TS 等待优雅退出，超过 6 秒终止后报告 cleanup_unconfirmed，不伪称清理成功。
