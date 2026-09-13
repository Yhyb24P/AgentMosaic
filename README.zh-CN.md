# research-agent-system

Rust v2 异构 coding-agent 团队：推理 Agent 与本地 worker 通过持久化 SQLite
team board 协作，结果、消息和 artifact 自动回流，不需要人工复制粘贴。

## 正常路径

```bash
cargo run -p agent-code-cli -- submit ./team.db bulk "修复任务"
cargo run -p agent-code-cli -- status ./team.db
cargo run -p agent-code-tui -- ./team.db
```

CLI/TUI 是 Rust 默认产品路径，不启动 Python daemon。完整 runtime 配置、恢复、
取消、artifact 和 final-result 命令见 [`README.md`](README.md)。旧 Python
control-plane 和 qualification 框架已从产品树移除，历史内容保留在 Git 历史。
