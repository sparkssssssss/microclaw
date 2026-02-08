# MicroClaw

MicroClaw is a Rust Telegram bot that connects Claude AI to Telegram with agentic tool execution, web search, scheduled tasks, and persistent memory. Inspired by [nanoclaw](https://github.com/gavrielc/nanoclaw/) (TypeScript/WhatsApp), incorporating some of its design ideas.

## Tech stack

Rust 2021, Tokio, teloxide 0.17, serenity 0.12, Anthropic Messages API (direct HTTP via reqwest), SQLite (rusqlite bundled), cron crate for scheduling.

## Project layout

- `src/main.rs` -- entry point, CLI
- `src/config.rs` -- YAML config loading (with .env backward compat)
- `src/error.rs` -- error types (thiserror)
- `src/telegram.rs` -- message handler, agentic loop, session resume, context compaction, typing indicator, catch-up
- `src/discord.rs` -- Discord bot (serenity gateway, reuses process_with_claude)
- `src/claude.rs` -- Anthropic API client, request/response types
- `src/db.rs` -- SQLite: chats, messages, scheduled_tasks, sessions tables
- `src/memory.rs` -- CLAUDE.md memory system (global + per-chat)
- `src/scheduler.rs` -- background task scheduler (60s polling)
- `src/tools/mod.rs` -- Tool trait, ToolRegistry (17 tools), ToolRegistry::new_sub_agent (9 restricted tools)
- `src/tools/path_guard.rs` -- sensitive path blacklisting for file tools
- `src/tools/bash.rs` -- shell commands
- `src/tools/read_file.rs`, `write_file.rs`, `edit_file.rs` -- file operations (with path guard)
- `src/tools/glob.rs`, `grep.rs` -- file/content search (with path guard filtering)
- `src/tools/memory.rs` -- read_memory, write_memory
- `src/tools/web_search.rs` -- DuckDuckGo search
- `src/tools/browser.rs` -- headless browser automation (agent-browser CLI wrapper)
- `src/tools/web_fetch.rs` -- URL fetching with HTML stripping
- `src/tools/send_message.rs` -- mid-conversation Telegram messaging
- `src/tools/schedule.rs` -- 5 scheduling tools
- `src/tools/sub_agent.rs` -- sub-agent tool with restricted tool registry

## Key patterns

- **Agentic loop** in `telegram.rs:process_with_claude`: call Claude -> if tool_use -> execute -> loop (up to MAX_TOOL_ITERATIONS)
- **Session resume**: full `Vec<Message>` (including tool_use/tool_result blocks) persisted in `sessions` table; on next invocation, loaded and appended with new user messages. `/reset` clears session.
- **Context compaction**: when session messages exceed `MAX_SESSION_MESSAGES`, older messages are summarized via Claude and replaced with a compact summary + recent messages kept verbatim
- **Sub-agent**: `sub_agent` tool spawns a fresh agentic loop with 9 restricted tools (no send_message, write_memory, schedule, or recursive sub_agent)
- **Tool trait**: `name()`, `definition()` (JSON Schema), `execute(serde_json::Value) -> ToolResult`
- **Shared state**: `AppState` in `Arc`, tools hold `Bot` / `Arc<Database>` as needed
- **Group catch-up**: `db.get_messages_since_last_bot_response()` loads all messages since bot's last reply
- **Scheduler**: `tokio::spawn` loop, polls DB for due tasks, calls `process_with_claude` with `override_prompt`
- **Typing**: spawned task sends typing action every 4s, aborted when response is ready
- **Path guard**: sensitive paths (.ssh, .aws, .env, credentials, etc.) are blocked in file tools via `path_guard` module
- **Multi-platform**: Telegram (primary), WhatsApp (webhook), Discord (serenity gateway) — all reuse `process_with_claude`

## Build & run

```sh
cargo build
cargo run -- start    # requires config.yaml with telegram_bot_token, api_key, bot_username
cargo run -- setup    # interactive setup wizard to create config.yaml
cargo run -- help
```

## Configuration

MicroClaw uses `config.yaml` (or `config.yml`) for configuration. Override the path with `MICROCLAW_CONFIG` env var. See `config.example.yaml` for all available fields. Legacy `.env` files are still supported with a deprecation warning.

## Adding a tool

1. Create `src/tools/my_tool.rs` implementing the `Tool` trait
2. Add `pub mod my_tool;` to `src/tools/mod.rs`
3. Register in `ToolRegistry::new()` with `Box::new(my_tool::MyTool::new(...))`

## Database

Four tables: `chats`, `messages`, `scheduled_tasks`, `sessions`. SQLite with WAL mode. Access via `Mutex<Connection>` in `Database` struct, shared as `Arc<Database>`.

## Important conventions

- All timestamps are ISO 8601 / RFC 3339 strings
- Cron expressions use 6-field format (sec min hour dom month dow)
- Messages are stored for all chats regardless of whether bot responds
- In groups, bot only responds to @mentions
- Consecutive same-role messages are merged before sending to Claude API
- Responses > 4096 chars are split at newline boundaries (Telegram), > 2000 chars for Discord
