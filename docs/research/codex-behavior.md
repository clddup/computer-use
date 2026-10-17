# Codex 行为研究与独立能力规格

本文件记录公开可描述的交互行为，不包含专有源码、私有 API 参数全集或目标执行器。本项目 wire 协议与实现均独立，不承诺与 Codex 私有协议兼容。

## 证据等级

- **文档契约**：安装版 computer-use guidance/API/confirmations 明确要求的调用顺序或安全边界。
- **客户端静态证据**：对安装版 `@oai/sky` 的可读 JS 客户端定向 REA 结果；能证明封装、校验、生命周期处理，不能单独证明原生 helper 的算法。
- **原生线索**：既有定向 PE/文本证据；能证明导入/资源/字符串存在，不能替代运行验证。
- **独立设计**：本项目为安全、跨平台和可验证性做出的选择，不声称 Codex 内部使用相同机制。

本轮 REA evidence：`ev_11fd2c9942d95ed894ab0a256d112bff6caf4f7b2dd61e838c7aacedfe6811fe`。使用 `rea-agents@6.5.0` 对明确入口的 7 个源文件做静态解析：33,084 字节、9,149 AST 节点、0 parse failures；223 个 inventoried 文件未选入。目标 JS/native helper 未执行。

## Codex 能力矩阵

| 能力链路 | 安装版行为/证据 | 本项目当前状态 | 处置与验收 |
| --- | --- | --- | --- |
| 应用发现与稳定标识 | `list_apps` 返回 app id、运行状态、窗口；guidance 要先发现再选择 | 仅 `allowed_pids`，要求应用已由宿主启动 | 增加宿主批准的 app scope、`list_apps`；不把标题/PID 当稳定 app id |
| 冷启动 | 未发现应用可用批准的显式应用路径启动，之后 refresh/list/select；启动器退出不等于窗口 ready | `Start` 只枚举已有批准 PID 的可见窗口 | 增加固定 launch profile 与 `launch_app`，按身份重新枚举并报告 readiness；短命 launcher fixture |
| 窗口选择 | `list_windows`、`get_window({id,app})`；恰好一个目标，否则列候选不猜 | 有 `list_windows`，无 get/activate；Windows 主动排除 owned 窗口 | 增加获取/激活与 owner/modal 关系；旧观察在重新选择后失效 |
| 激活与焦点 | `activate_window` 后再取 state；typing 前看 focused element | 仅动作前 foreground/几何复核，未暴露焦点状态 | Observation 增加 focus/selection/document context；焦点未知不冒充编辑焦点 |
| 观察默认值 | 默认 screenshot=true、text/accessibility 按需；state 可同时返回 accessibility 与 screenshots | SDK 默认 screenshot/accessibility 都 true | 将 accessibility 默认改为 false；显式报告未请求的数据为空/未知 |
| 图像 | screenshots[] 可有 origin、尺寸、zIndex，多区域可单独投递给模型 | 单一主窗口 Image，已有像素映射和真实 PNG | 保留每图独立映射；有 owner/modal 证据后才扩展多区域，不伪造完整桌面 |
| 辅助功能上下文 | focused_element、selected_text、selected_elements、document_text、tree | 只有扁平 elements；密码值过滤 | 三平台在能力可用时填充结构化字段，受长度/节点/权限限制 |
| 点击与滚动 | click 支持 element/point、click_count、mouse_button；scroll 为窗口坐标注入 | 单击、三种 button、元素/图片 target；无 click_count | 增加有界 click_count，动作前后回归 oracle；scroll 保留坐标语义 |
| 按键和文字 | `press_key` 支持 keysym/标点/numpad 别名；`type_text` 是 literal text，Enter/Tab 单独 press | 仅窄 Windows key 名；平台能力不同 | 共享规范和平台映射；先校验整组，不能把控制字符混入 literal text |
| 设值与次级动作 | `set_value`、`perform_secondary_action` 有独立客户端契约；具体控件能力由原生返回决定 | wire 有 set_value，Windows/macOS 尚 unsupported，Linux 有 EditableText | 仅在真实 ValuePattern/AX/AT-SPI 路径宣告；secondary 使用平台白名单 |
| 应用级审批 | client/helper 可发 approval request，由 trusted host elicitation 接受；模型不能自授权 | immutable PID/capture/input policy，无交互审批 adapter | 增加可选可信宿主 approval 边界；审批不可用即拒绝，不接受模型 approval 字段 |
| 敏感边界 | guidance/confirmations 禁止通过桌面内容扩大权限，要求锁屏/安全桌面停用 | policy 是约束层而非 OS sandbox | 保持默认拒绝、最小 scope；文档明确删除/外发/支付等需上层确认 |
| 中止与本轮生命周期 | helper transport 处理物理 Escape 标记、停止本 turn 后拒绝继续；turn 切换会结束旧状态 | stop/EOF/TTL 已有，缺物理 Escape/turn API | 增加宿主 AbortSignal/turn-end；中止会话终止，不自动恢复/重放 |
| 超时/断线 | launch 有更长预算；请求超时关闭 helper；approval 等待暂停请求预算 | act timeout unknown、不重放；method 识别仍偏向 act | 所有有副作用方法统一 unknown/dedupe；审批等待与执行 timeout 分开 |
| 图像交付 | client 将 screenshot URL 转模型 image content | MCP 返回真实 image，SDK 返回 base64 image | 保持 stdio 与图片元数据分离；不把桌面内容当指令 |
| 音频/历史/私有桥 | base 中音频方法受 `SKY_ENABLE_AUDIO` 控制；使用历史/Node bridge 是目标宿主细节 | 不实现音频、历史排序或私有 bridge | 显式排除，不把存在的方法误报为本项目核心桌面能力 |

## 本项目独立且已验证的可靠性

1. **窗口与观察引用**：先明确窗口；元素、图像和观察绑定会话、epoch、窗口几何与时效，动作最多消费一个观察。
2. **投递语义**：`rejected` 表示未投递，`dispatched` 仅表示 OS 接受，`unknown` 不自动重放；重新观察验证应用结果。
3. **停止**：stop/EOF/TTL 优先取消；输入 guard 释放已由项目持有的按键/鼠标。崩溃/强杀不保证系统一定释放。
4. **授权**：宿主 policy 默认拒绝，截图、辅助功能和输入分离；窗口数据不扩大授权。文件锁阻止多个 helper 同时控制同一用户桌面。
5. **隐私**：密码元素不读值；树、截图、输入文本不默认写诊断日志；没有任意 shell/JS 执行工具。

## 当前实现边界

Windows 自建 fixture 的 SDK/MCP 真实闭环已验证，但测试此前由 host 预先 spawn fixture 并按 PID 授权，**不覆盖 Codex 式冷启动**。Linux 仅有交叉编译检查，macOS 未在原生环境编译/运行。缺少证据的 Codex 原生行为保持 unknown，不用静态文件名推断完整实现。
