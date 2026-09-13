# AgentMosaic

[English](README.md)

[![CI](https://github.com/Yhyb24P/AgentMosaic/actions/workflows/rust.yml/badge.svg?branch=main)](https://github.com/Yhyb24P/AgentMosaic/actions/workflows/rust.yml)
[![Latest release](https://img.shields.io/github/v/release/Yhyb24P/AgentMosaic)](https://github.com/Yhyb24P/AgentMosaic/releases/latest)
[![License](https://img.shields.io/badge/license-Apache--2.0-blue)](LICENSE)

**把异构 coding Agent 作为一个持久化团队来运行。**

AgentMosaic 把高推理能力的 Lead、coding Agent、本地模型和确定性 worker 接到同一个项目上。
给团队一个目标：Lead 负责规划与委派，worker 负责执行，结果与 artifact 自动回流，供 Lead
评审与综合。

本地模型通过 ACP 兼容的 runtime 接入；AgentMosaic 本身不托管、也不选择模型。

不需要人工在 Agent 之间复制粘贴。

## 安装

```bash
curl --proto '=https' --tlsv1.2 -LsSf \
  https://am.yhshyp.xyz/install.sh | sh
```

```bash
am --version
```

预编译发行版目前只面向 Linux x86_64。如需自行构建二进制，见[源码构建](#源码构建)。

## 快速开始

```bash
am init

am agent add lead \
  --role reasoner \
  --adapter codex-app-server -- codex

am agent add worker \
  --role worker \
  --adapter acp -- qwen --acp

am doctor

am run "implement the task, verify it, and summarize the result"
```

`am init` 在 `.agentmosaic/state.db` 创建项目本地持久化状态，并把它排除在版本控制之外。
在项目内任意目录下，`am run` 都能发现这份状态。

`--` 之后的全部内容是不透明的 launch argv。AgentMosaic 只存储并原样执行，从不解释
launcher 专有参数，凭据也不应写在这里。

`am doctor` 在不做任何认证的前提下检查项目、团队与 runtime 的就绪状态；只要注册的
`reasoner` 不是恰好一个，它就会报告 `LEAD_SELECTION_AMBIGUOUS_OR_MISSING`。`am run`
需要这个唯一的 Lead 才能启动。

### 可选：增加一个 utility worker

utility Agent 的注册方式相同，用于有边界的工具型工作：

```bash
am agent add utility --role utility --adapter acp -- <program> --acp
```

本地 launcher 保留自己的 argv，例如：

```bash
am agent add lead-ds --role reasoner --adapter codex-app-server -- codex -ds
```

## 为什么需要 AgentMosaic？

手工串联两个 Agent 的流程是这样的：

| 手工 Agent 流程 | AgentMosaic |
|---|---|
| 推理模型做规划 | 给团队一个目标 |
| 你把指令复制到另一个 Agent | Lead 委派任务 |
| worker 执行完，你把结果复制回来 | worker 执行并自动回传结果 |
| 推理模型评审，然后你重复以上步骤 | Lead 跟进，最后持久化一个结果 |

这样，昂贵的高推理模型把预算花在规划、难题推理和综合上；coding Agent、本地模型和
确定性 worker 承担重复、长时间、文件密集和工具密集的工作。

你不再是 Agent 之间的传输通道；运行即使中断，状态依然可检查，而不是丢在一次对话里。

## 工作原理

```text
                    one objective
                         |
                         v
                  Lead / Reasoner
                 /      |       \
                v       v        v
             Agent    Agent    Worker
                \       |       /
                 +-- results ---+
                         |
                         v
                 review / follow-up
                         |
                         v
                  durable result
```

Lead 负责规划、难题推理、综合与评审。worker 完成 Lead 委派给它的任务。每个结果与
artifact 都落到持久化的 board 上，因此 Lead 可以继续跟进、要求修正，或用一次最终答复
结束目标。

## Runtime 边界

### AgentMosaic 负责

```text
roles
delegation
task state
result / artifact flow
bounded contracts
recovery
```

### 外部 runtime 负责

```text
login
credentials
provider
model
launcher profile
```

ACP 兼容的 coding runtime 通过一个有边界的 worker 边界与 AgentMosaic 通信。ACP driver
接收一个调度任务，返回有边界的结构化结果和 artifact 哈希；权威状态始终在 SQLite board 上。

Codex 是当前通过 `codex-app-server` 接入的参考高推理 Lead。它的 thread 在规划、跟进和
综合之间常驻，外部 thread/turn binding 会被持久化。任何满足同一能力边界的 Agent 或
runtime 都可以承担这个角色。

## 持久化与恢复

- 项目本地 SQLite 状态，不是内存会话。
- 委派任务、结果和 artifact 在产生的过程中即被持久化。
- Lead 的决策遵循一份签入仓库的严格契约，失败即关闭。
- 中断的运行可以恢复，且不会重放已经成功的工作。
- 已完成的工作不会被无条件重放。
- 检查类命令不会启动 runtime。

```bash
am status .agentmosaic/state.db
am final .agentmosaic/state.db <root-task-id>
am tui .agentmosaic/state.db
```

其余能力由 `am registry`、`am artifact`、`am binding`、`am recover`、`am recover-all`
和 `am resume-team` 覆盖，见[恢复](docs/recovery.md)。

## 文档

- [快速开始](docs/getting-started.md)
- [架构](docs/architecture.md)
- [CLI 参考](docs/cli.md)
- [恢复](docs/recovery.md)
- [Codex runtime](docs/runtimes/codex.md)
- [ACP runtime](docs/runtimes/acp.md)
- [Qwen Code runtime](docs/runtimes/qwen-code.md)
- [发行历史](docs/releases/v0.1.0.md) / [历史](docs/history.md)

## 源码构建

```bash
cargo build --release --workspace
```

产物为 `target/release/am`，即唯一随产品发布的二进制。它的 Codex 协作 bridge 是固定的
隐藏内部命令，不单独安装或配置。

提交改动前必须通过的检查：

```bash
scripts/ci/check_identity.sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --release --workspace
git diff --check
```

## 许可证

Apache License 2.0 (ALv2)。见仓库根目录的 `LICENSE` 文件。
