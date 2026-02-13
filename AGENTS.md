# AGENTS.md

## Project overview

MicroClaw is a Rust multi-channel agent runtime for Telegram, Discord, and Web.
It shares one channel-agnostic agent loop (`src/agent_engine.rs`) and one provider-agnostic LLM layer (`src/llm.rs`), with channel adapters for ingress/egress.

Core capabilities:
- Tool-using chat agent loop (multi-step tool calls)
- Session resume and context compaction
- Scheduled tasks + background scheduler
- File memory (`AGENTS.md`) + structured SQLite memory
- Memory reflector, quality gate, and observability metrics
- Skills + MCP tool federation

## Tech stack

- Language: Rust (edition 2021)
- Async runtime: Tokio
- Telegram: teloxide
- Discord: serenity
- Web API/UI: axum + React (in `web/`)
- Database: SQLite (rusqlite)
- LLM runtime: provider abstraction with native Anthropic and OpenAI-compatible providers

## Source index (`src/`)

- `main.rs`: CLI entry (`start`, `setup`, etc.)
- `runtime.rs`: app wiring (`AppState`), provider/tool initialization, channel boot
- `agent_engine.rs`: shared agent loop (`process_with_agent`), explicit-memory fast path, compaction, tool loop
- `llm.rs`: provider implementations + stream handling + format translation
- `llm_types.rs`: model/tool/message DTOs
- `channels/telegram.rs`: Telegram adapter
- `channels/discord.rs`: Discord adapter
- `channels/delivery.rs`: cross-channel outbound delivery helpers
- `channel.rs`: channel abstraction types
- `web.rs`: Web API routes, stream APIs, config/usage endpoints
- `db.rs`: SQLite schema, migrations, chat/session/task/memory persistence
- `memory.rs`: file-memory manager (`runtime/groups/.../AGENTS.md`)
- `memory_quality.rs`: explicit remember parser, normalization, quality rules, topic-key heuristics
- `scheduler.rs`: scheduled-task runner + memory reflector loop
- `usage.rs`: token/cost/memory usage report assembly
- `embedding.rs`: optional runtime embedding providers (for `sqlite-vec` flows)
- `skills.rs`: skill discovery/activation
- `builtin_skills.rs`: bundled skill materialization
- `mcp.rs`: MCP server/tool integration
- `gateway.rs`: event stream / request lifecycle infra
- `setup.rs`: interactive setup wizard and provider presets
- `doctor.rs`: environment diagnostics
- `tools/`: built-in tool implementations and registry

## Tool system

`src/tools/mod.rs` defines:
- `Tool` trait (`name`, `definition`, `execute`)
- `ToolRegistry` dispatch and auth context injection
- risk/approval gate for high-risk tools in sensitive contexts

Current built-in tools are generated from code to avoid drift:
- `docs/generated/tools.md`
- `website/docs/generated-tools.md`

Regenerate docs artifacts with:
```sh
node scripts/generate_docs_artifacts.mjs
```

## Agent loop (high level)

`process_with_agent` flow:
1. Optional explicit-memory fast path (`remember ...`/`记住...`) writes structured memory directly
2. Load resumable session from `sessions`, or rebuild from chat history
3. Build system prompt from file memory + structured memory context + skills catalog
4. Compact old context if session exceeds limits
5. Call provider with tool schemas
6. If `tool_use`: execute tool(s), append results, loop
7. If `end_turn`: persist session and return text

## Memory architecture

Two layers:

1. File memory:
- Global: `runtime/groups/AGENTS.md`
- Chat: `runtime/groups/{chat_id}/AGENTS.md`

2. Structured memory (`memories` table):
- category, confidence, source, last_seen, archived lifecycle
- explicit remember fast path
- reflector extraction from conversation history
- dedup/supersede handling with `memory_supersede_edges`

Observability tables:
- `memory_reflector_runs`
- `memory_injection_logs`

Surfaces:
- `/api/usage` summary block
- `/api/memory_observability` time-window series API
- Web Usage Panel trends/cards

## Database

`db.rs` includes:
- schema creation + schema-version migrations (`db_meta`, `schema_migrations`)
- chat/message/session/task persistence
- structured memory CRUD + archive/supersede
- usage and memory observability queries

## Web/API

`web.rs` routes include:
- chat send/send_stream + SSE stream replay
- sessions/history/reset/delete
- config read/update
- usage text report (`/api/usage`)
- memory observability series (`/api/memory_observability`)

## Build and test

```sh
cargo build
cargo test
npm --prefix web run build
npm --prefix website run build
```

Docs drift guard (CI + local):
```sh
node scripts/generate_docs_artifacts.mjs --check
```
