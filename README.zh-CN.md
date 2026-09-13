# research-agent-system

Rust v2 异构 coding-agent 团队：推理 Agent 与本地 worker 通过持久化 SQLite
team board 协作，结果、消息和 artifact 自动回流，不需要人工复制粘贴。

Codex 是参考的高智能 Lead，Qwen Code 是参考的 Worker。产品路径是
「一个目标进，一个持久化的团队结果出」：

```text
配置/注册 Codex Lead
配置/注册 Qwen Worker（以及一个 utility agent）
run-team 一个目标
status            （只读的持久化团队面板）
final / artifact  （持久化最终答案及其精确引用）
recover / resume  （中断后恢复）
```

`submit` 只创建一个 pending 看板任务，不是一次团队运行；`run-team` 才是团队
入口。所有命令直接操作权威 SQLite board，不启动旧 Python。

`register` 字段语法（尾部 8 到 10 个字段）：

```text
agent-code-cli register <database> <agent-id> <name> <tier> <driver-kind> <executable> <driver-args> <max-concurrency> <tags> [<runtime-version-or->] [<driver-config-json-or->]
```

- `tier` 为 `reasoner`、`worker` 或 `utility`。
- `driver-kind` 为 `native`、`acp`、`cli`、`codex-app-server` 或 `-`。
- `driver-args` 与 `tags` 用逗号分隔；无内容用 `-`。
- `runtime-version` 是可选的第 9 个字段；无内容用 `-`。
- 可选的第 10 个字段是一个非机密 JSON 对象，或 `-`。看起来像凭据的键
  （`token`、`key`、`secret`、`password`、`endpoint`）会被拒绝，因此凭据不可能
  经此写入。
  - `acp`：`auth_method`、`timeout_seconds`、`max_prompt_bytes`、
    `max_result_bytes`、`artifact_paths`。
  - `codex-app-server`：`mcp_command`（必需，必须是一个已存在的文件，即构建出的
    `ras_codex_mcp`）、`artifact_paths`、`max_events`、`overrides`；当该 agent 是
    本次运行的 Lead 时还会读取 `model`、`max_prompt_bytes`、`max_answer_bytes`。

先构建 workspace，使 `ras_codex_mcp` 与 `agent-code-cli` 存在，并把
`mcp_command` 指向构建出的 bridge 的绝对路径：

```bash
cargo build --release --workspace
```

```bash
# 1. 注册 Codex Lead（参考 Reasoner，codex-app-server 驱动）
cargo run -p agent-code-cli -- register ./team.db codex-lead codex-lead reasoner \
  codex-app-server codex - 1 codex,lead - \
  '{"mcp_command":"/abs/path/to/target/release/ras_codex_mcp","model":"gpt-5.5","max_events":200,"overrides":["model=\"gpt-5.5\"","model_reasoning_effort=\"low\""]}'

# 2. 注册 Qwen Code Worker 与 utility agent（ACP 驱动）
cargo run -p agent-code-cli -- register ./team.db qwen-worker qwen-worker worker \
  acp qwen --acp 1 - - \
  '{"auth_method":"openai","timeout_seconds":600,"artifact_paths":["worker.txt"]}'
cargo run -p agent-code-cli -- register ./team.db qwen-utility qwen-utility utility \
  acp qwen --acp 1 - - '{"auth_method":"openai","timeout_seconds":600}'

# 3. 用一个目标跑完整的团队流程
cargo run -p agent-code-cli -- run-team ./team.db /path/to/repo "生成 worker.txt 并总结"

# 4. 只读的持久化看板视图（不启动 runtime）
cargo run -p agent-code-cli -- status ./team.db
cargo run -p agent-code-cli -- registry ./team.db
cargo run -p agent-code-tui -- ./team.db          # 只读面板；q 退出

# 5. 持久化最终答案及其精确引用
cargo run -p agent-code-cli -- final ./team.db 1
cargo run -p agent-code-cli -- artifact ./team.db 2
cargo run -p agent-code-cli -- binding ./team.db 2

# 6. 中断后：先关闭中断的 attempt，再 resume
cargo run -p agent-code-cli -- recover-all ./team.db
cargo run -p agent-code-cli -- resume-team ./team.db /path/to/repo 1
```

`run-team` 与 `resume-team` 都接受：

```text
--lead <agent-id>   注册了多个 reasoner 时指定 Lead
--max-rounds N      Lead 推理轮数上限
--max-tasks N       委派任务预算上限
--max-retries N     单 agent 在重新指派前的重试上限
```

不使用 `--lead` 时，必须恰好注册一个 `reasoner`；0 个或多个都会失败，而不是猜测。

`run-team` 打开/迁移 board，读取持久化 agent registry，构建经过校验的 registry，
解析 Lead，构造真实 driver，创建一个持久化的根 `reasoning` 任务及其 Lead attempt，
并通过 `Lead` + `Scheduler` 运行常驻的 Codex `CodexLeadBrain`。委派任务通过 ACP 在
真实的 Qwen worker 上执行，最终可见的 Codex 答案与精确选中的 task/artifact 引用会
持久化到根任务上。`resume-team` 从持久化状态重建 driver/brain，关闭中断的后代任务
而不重放它们，对已经成功的根任务是幂等的。

Lead 的决策是严格 JSON，由产品校验；不符合契约的决策在至多一次有界纠正轮后
fail-closed。决策 wire 契约见
[`contracts/lead_decision.schema.json`](contracts/lead_decision.schema.json)。

只读命令 `status`、`registry`、`artifact`、`binding`、`final` 以及 TUI 面板都不会
启动 driver 或改变 board 的 runtime 状态。`status` 会为每个任务打印
`parent=<id|->`。完整 runtime 配置、恢复、取消、artifact 与最终结果等命令见
[`README.md`](README.md)。旧 Python control-plane 和 qualification 框架已从产品树
移除，历史内容保留在 Git 历史。
