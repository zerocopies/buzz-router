# buzz-router

**HTTP governance layer for local AI inference.**  
Sits between your application and a running inference engine, enforcing session budgets, capability policies, and request boundaries — so you control exactly who gets what, and how much.

Built in Rust. Pairs with [qfz3](https://github.com/zerocopies/qfz3), the zero-copy inference engine. Works with any HTTP-based inference backend.

---

## What It Does

Most local inference setups give you raw model access with no controls. buzz-router adds a governance layer:

- **Session management** — each client gets an isolated session with its own token budget
- **Budget enforcement** — requests are checked and reserved before inference runs; committed on success, cancelled on failure
- **Capability policies** — define what each session is allowed to do (which models, which endpoints, which features)
- **Privacy boundaries** — outbound capability calls are audited before execution
- **Multi-model routing** — register multiple models and route requests by name

No cloud dependency. No telemetry. Runs entirely on your machine.

---

## Architecture

```
Client Request
      ↓
  buzz-router (port 7474)
      ↓
  BoundaryEnforcer
  ├── check_and_reserve()     ← validates capability + locks budget
  ↓
  ExecutionEngine
  ├── spawn_blocking (inference)
  ↓
  On success → commit_execution(actual_tokens, actual_cost)
  On failure → cancel_execution(reserved_tokens, reserved_cost)
      ↓
  SessionManager (DashMap, thread-safe)
      ↓
  qfz3 inference engine (via HTTP)
```

---

## Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/chat` | Send a message, get a response |
| GET | `/sessions` | List all active sessions |
| GET | `/sessions/:id` | Get session detail and budget usage |
| POST | `/new_session` | Create a new session |
| GET | `/models` | List registered models |
| GET | `/status` | Health check |
| GET | `/stats` | Aggregate usage statistics |

---

## Quickstart

**Requirements:** Rust 1.75+, a running qfz3 instance (or any HTTP inference backend)

```bash
# Clone
git clone https://github.com/zerocopies/buzz-router
cd buzz-router

# Configure
cp .env.example .env
# Edit .env — set your inference engine URL and model paths

# Build
cargo build --release

# Run
./target/release/buzz_router
# Listening on 0.0.0.0:7474
```

**Send a request:**

```bash
curl -X POST http://localhost:7474/chat \
  -H "Content-Type: application/json" \
  -d '{
    "session_id": "my-session",
    "message": "What is the capital of France?",
    "model": "qfz3"
  }'
```

---

## Configuration

Set via `.env` or environment variables:

```env
INFERENCE_ENGINE_URL=http://localhost:8080
DEFAULT_TOKEN_BUDGET=10000
DEFAULT_COST_BUDGET=1.00
SESSION_IDLE_TIMEOUT_SECS=3600
MAX_CONCURRENT_SESSIONS=100
LOG_LEVEL=info
```

---

## Session Budgets

Every session tracks two limits:

- **Token budget** — total tokens (input + output) allowed for this session
- **Cost budget** — estimated cost ceiling in your chosen unit

The reservation lifecycle prevents race conditions on concurrent requests:

```
try_reserve_budget()    → atomic check + lock
commit_reservation()    → deduct actual usage
cancel_reservation()    → release on failure (no charge)
```

Concurrent requests to the same session cannot double-spend.

---

## Capability Policies

Define what a session can access:

```json
{
  "session_id": "restricted-session",
  "capabilities": ["chat", "summarise"],
  "models_allowed": ["qfz3"],
  "max_tokens_per_request": 500
}
```

Requests for capabilities not in the list are rejected at the boundary before inference runs. No wasted compute.

---

## Project Structure

```
buzz-router/
├── src/
│   ├── main.rs              # Entry point, server init
│   ├── server/
│   │   └── mod.rs           # Axum HTTP handlers
│   ├── session/
│   │   └── mod.rs           # SessionManager + Budget
│   ├── boundary/
│   │   └── enforcer.rs      # BoundaryEnforcer
│   ├── engine/
│   │   └── mod.rs           # ExecutionEngine + inference wiring
│   ├── capabilities/
│   │   └── mod.rs           # CapabilityRegistry
│   └── types.rs             # Shared types
├── Cargo.toml
├── .env.example
└── README.md
```

---

## Pair With qfz3

buzz-router is designed to sit in front of [qfz3](https://github.com/zerocopies/qfz3) — a from-scratch Rust LLM inference engine with zero-copy mmap weight loading.

```
~/
├── qfz3/          # Inference engine — loads and runs the model
└── buzz-router/   # Governance layer — controls access to qfz3
```

```toml
# buzz-router/Cargo.toml
[dependencies]
qfz3 = { path = "../qfz3" }
```

They can also run independently — buzz-router works with any HTTP inference backend.

---

## Known Limitations

- **SQLite not used** — session state is in-memory only (DashMap). State is lost on restart. Persistence layer planned.
- **Budget reservation bug** — under high concurrency, the commit/cancel pattern in `engine/mod.rs` has a known edge case where the reservation is committed before inference completes. Fix in progress.
- **No authentication** — buzz-router does not handle API keys or auth. Put it behind a reverse proxy (nginx, caddy) if exposing beyond localhost.
- **Single process** — no distributed session support. Designed for single-machine deployment.

---

## Roadmap

- [ ] Fix budget reservation race condition in `engine/mod.rs`
- [ ] SQLite persistence for session state (survive restarts)
- [ ] API key middleware
- [ ] Prometheus metrics endpoint
- [ ] Rate limiting per session
- [ ] WebSocket support for streaming responses

---

## Built By

[zerocopies](https://github.com/zerocopies) — building portable, self-hosted AI infrastructure.

Part of the Zero Copies stack:
- **qfz3** — zero-copy inference engine
- **buzz-router** — HTTP governance layer
- **Crossway** — agent orchestration framework

---

## License

MIT
