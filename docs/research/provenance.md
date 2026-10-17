# 研究来源、指纹与合规范围

记录日期：2026-10-17。目标是用户自己安装的 Codex Windows 发行版的互操作理解和独立能力设计。不是严格 clean-room（已阅读目标可读实现），也不是目标文件再分发授权。

## 核对

- 本机 Appx：OpenAI.Codex **26.1013.3656.0**。
- computer-use 插件：**26.1013.31256**；@oai/sky **0.7.8**、@oai/cua **0.2.6**、@oai/cua-repl **0.1.0**。
- 安装路径、两份既有 Temp 静态证据目录以及交接点名的 11 个文件均已核对存在且非空；没有重复全面扫描。
- 本轮没有重新计算全部原生二进制哈希；**版本号一致不代表已证明字节一致**。
- 目标插件标记 Proprietary。独立仓库不复制这些实现、归档或二进制。

## REA

官方项目：[morluto/rea](https://github.com/morluto/rea)。固定工具包 `rea-agents@6.5.0`，从官方 npm registry 安装到本地 `.local/rea-tool`。安装使用 ignore-scripts、omit optional，没有运行 setup、Frida 或注入。研究使用 Node 24.14.0；工具自身是 MIT，不等于目标内容也是 MIT。

### 既有单文件结果

此前对 `computer_use_client.js` 的单文件结果仍保留，能证明一层 client transport 封装，不代表完整 computer-use 能力：

| 项目 | 记录 |
| --- | --- |
| evidence_id | ev_f36eb110eb44f10ed64b039ce423c96adc11053a8697b45c0c3457f8f1ab5790 |
| graph nodes | 456 |
| selected source size | 7646 bytes |
| parse / semantic coverage | complete / complete；未截断 |
| 其他文件 | 229 个仅 inventoried，不是 229 文件全量 source analysis |

### 本轮七文件定向结果

为核对方法映射、应用生命周期、审批/turn 元数据和 helper transport，重新执行 `analyze-javascript-application`，只选择已知入口：

```text
dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client_base.js
dist/project/cua/sky_js/src/targets/windows/internal/computer_use_client.js
dist/project/cua/sky_js/src/targets/windows/internal/helper_transport.js
dist/project/cua/sky_js/src/targets/windows/internal/codex_turn_metadata.js
dist/project/cua/sky_js/src/service.js
dist/project/cua/sky_js/src/sky.js
dist/project/cua/sky_js/src/create_client.js
```

使用 `--artifact-format directory --max-semantic-nodes 20000 --max-heap-mb 512 --analysis-timeout-ms 30000 --format json`，REA exit code **0**。证据：

| 项目 | 记录 |
| --- | --- |
| evidence_id | `ev_11fd2c9942d95ed894ab0a256d112bff6caf4f7b2dd61e838c7aacedfe6811fe` |
| root artifact SHA256（REA 报告） | `6e5dfe6d6dfa85d5a1f3786bd1647ec8623d357de07dd4def5f07b6a3dad21c8` |
| selected source files / bytes | 7 / 33,084 |
| visited AST nodes | 9,149 |
| parse failures | 0 |
| other inventoried files | 223，未分析 |

该分析以静态文本方式完成，不执行 JavaScript、HTML、Electron bootstrap、目标 helper 或原生扩展。静态 import、入口和关系不证明运行时一定执行；动态表达式/未选文件关系保持 unknown。原始 JSON 结果保留在会话工具输出和本机被忽略目录，不随项目分发。

两个结果的 root artifact 指纹只按 REA 报告语义引用，不将其冒充所有原生目标二进制哈希。

## 数据处理

下载/提取目标按不可信数据处理，各自新目录；自写脚本与目标分开，不从目标提取目录执行解释器/构建工具。没有运行目标私有 helper，没有 bypass 登录/权益/审批，没有读取聊天、cookies、账号 token 或其他凭据，没有将目标文件上传外部服务。

本仓库保存的是脱敏行为描述、独立实现及自建测试，不保存专有源码片段。旧 ZCode 证据仅作为既有交接来源核验，本轮没有新增全面调研。
