# AgentMosaic

[English](README.md)

AgentMosaic（`AM`）是一个**异构 Agent coding/work 团队**。一个目标进，一个持久化的
团队结果出。

唯一职责：把不同长处的 Agent 接到同一个项目上。高智能 Agent 负责规划、难题推理、
架构、综合与评审；本地或低成本 Agent 与确定性 worker 负责重复、长时间、文件密集、
数据密集和工具密集的工作。结果与 artifact 自动回流到继续推理的那个 Agent，不需要
人工在 Agent 之间复制粘贴。

通信、调度、恢复与安全边界是让多个 Agent 完成工作的支撑机制，它们不是产品本身。

旧产品身份与稳定的 `v0.1.0` 发行版说明见 [docs/history.md](docs/history.md)。

## 架构

```text
User
  |
  v
Team Session
  |
  v
Lead / Reasoning Agent
  | delegate
  +------------------+-------------------+
  v                  v                   v
Reasoning Agent    Local Model Agent   Utility Worker
  |                  |                   |
  +----- result / files / messages ------+
                       |
                       v
              Lead integrates result
                       |
                       v
                    Deliver
```

每个原生 model-backed Agent 运行同一个内部循环：

```text
Init -> Observe -> Model Decision -> Tool Execution -> Observe -> ...
     -> Verify -> Deliver / Rollback
```

## 构建

```bash
cargo build --release --workspace
```

产物为 `target/release/am`，即唯一的产品二进制。Codex MCP bridge 是固定的隐藏内部
命令，不单独配置或安装。

## 快速开始 — 一个异构团队目标

一个目标进，一个持久化的团队结果出。AgentMosaic 管理角色、机器协议和 LaunchSpec；
外部 runtime 管理自己的登录、凭据、provider、模型和 launcher profile。

```bash
# 1. 初始化项目本地状态
am init

# 2. 注册外部 runtime 的不透明 argv
am agent add lead --role reasoner --adapter codex-app-server -- codex
am agent add worker --role worker --adapter acp -- qwen --acp
am agent add utility --role utility --adapter acp -- aweswitch qw --acp

# 3. 不管理认证地检查就绪状态
am doctor

# 4. 用一个目标跑完整的团队流程
am run "生成 worker.txt 并总结"
```

`run-team` 与 `resume-team` 都接受：

```text
--lead <agent-id>   注册了多个 reasoner 时指定 Lead
--max-rounds N      Lead 推理轮数上限
--max-tasks N       委派任务预算上限
--max-retries N     单 agent 在重新指派前的重试上限
```

高级兼容接口 `register` 的字段语法（尾部 8 到 10 个字段）：

```text
am register <database> <agent-id> <name> <tier> <driver-kind> <executable> <driver-args> <max-concurrency> <tags> [<runtime-version-or->] [<driver-config-json-or->]
```

- `tier` 为 `reasoner`、`worker` 或 `utility`。
- `driver-kind` 为 `native`、`acp`、`cli`、`codex-app-server` 或 `-`。
- `driver-args` 与 `tags` 用逗号分隔；无内容用 `-`。
- `runtime-version` 是可选的第 9 个字段；无内容用 `-`。
- 可选的第 10 个字段是一个非机密 JSON 对象，或 `-`。看起来像凭据的键
  （`token`、`key`、`secret`、`password`、`endpoint`）会被拒绝，因此凭据不可能
  经此写入。
  - 旧 ACP `auth_method` 与旧 Codex `mcp_command` 仅为已有 v11 board 的兼容字段；
    正常 onboarding 不写入它们。新产品路径自动使用内部 MCP bridge。

`submit` 只创建一个 pending 看板任务，不是一次团队运行；`run-team` 才是团队入口。
所有命令直接操作权威 SQLite board。

### 本地/自定义 launcher 示例

AgentMosaic 不解释 launcher 专有参数：它只保存并原样执行 `--` 之后的 argv。
例如，本地 Codex shim 可以注册为 `codex -ds`；兼容 ACP 的 wrapper 也可保留自己的 argv。

```bash
am agent add lead-ds --role reasoner --adapter codex-app-server -- codex -ds
am agent add utility --role utility --adapter acp -- aweswitch qw --acp
```

## 团队运行流程

`am run-team` 打开/迁移 board，读取持久化 agent registry，构建经过校验的 registry，
解析 Lead，构造真实 driver，创建一个持久化的根 `reasoning` 任务及其 Lead attempt，
并通过 `Lead` + `Scheduler` 运行常驻的 Codex `CodexLeadBrain`。委派任务通过 ACP 在
真实的 Qwen worker 上执行，最终可见的 Codex 答案与精确选中的 task/artifact 引用会
持久化到根任务上。`am resume-team` 从持久化状态重建 driver/brain，关闭中断的后代任务
而不重放它们，对已经成功的根任务是幂等的。

Lead 的决策是严格 JSON，由产品校验；不符合契约的决策在至多一次有界纠正轮后
fail-closed。决策 wire 契约见
[`contracts/lead_decision.schema.json`](contracts/lead_decision.schema.json)。

只读命令 `status`、`registry`、`artifact`、`binding`、`final` 以及 `tui` 面板都不会
启动 driver 或改变 board 的 runtime 状态。

## 文档

- [架构](docs/architecture.md)
- [快速开始](docs/getting-started.md)
- [CLI 参考](docs/cli.md)
- [恢复](docs/recovery.md)
- [Runtime](docs/runtimes/acp.md)：[Codex](docs/runtimes/codex.md)、[Qwen Code](docs/runtimes/qwen-code.md)
- [历史](docs/history.md)

## Rust 开发

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --workspace
git diff --check
```

## 许可证

Apache License 2.0 (ALv2)。见仓库根目录的 `LICENSE` 文件。
