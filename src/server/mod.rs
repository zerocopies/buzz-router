// server/mod.rs
//
// FIX: Removed redundant deduct_budget call from handle_chat.
// The boundary enforcer (called inside engine.process_with_tools via
// boundary.execute) already does a full reserve → commit cycle for
// estimated_tokens. Calling deduct_budget afterward was a second
// deduction on every request, causing the ~507 token double-hit.
//
// What happens now per request:
//   boundary.execute()  →  try_reserve_budget(500) + commit_reservation(500)
//   handle_chat         →  delta deduct only if actual > estimated
//
// No other changes from the original file.

use crate::engine::tools::ToolRegistry;
use crate::engine::{EngineRequest, ExecutionEngine};
use crate::memory::MemoryStore;
use crate::session::SessionManager;
use crate::types::{PrivacyLevel, SessionId};
use axum::{
    extract::{Path, State},
    routing::{get, post},
    Json, Router,
    response::{Html, IntoResponse, Response},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{error, info, warn};

#[derive(Debug, Deserialize)]
pub struct ChatRequest {
    pub session_id: Option<String>,
    pub message: String,
    pub capability_id: String,
    pub privacy: Option<PrivacyLevel>,
    pub token_budget: Option<u64>,
    pub cost_budget: Option<f64>,
    pub estimated_tokens: Option<u64>,
    pub estimated_cost: Option<f64>,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ChatResponse {
    pub session_id: String,
    pub output: String,
    pub tokens_used: u64,
    pub cost_incurred: f64,
    pub status: String,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub privacy: Option<PrivacyLevel>,
    pub token_budget: Option<u64>,
    pub cost_budget: Option<f64>,
}

#[derive(Debug, Serialize)]
pub struct CreateSessionResponse {
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub struct FactRequest {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct FactResponse {
    pub status: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub struct LoadModelRequest {
    pub path: String,
}

#[derive(Debug, Serialize)]
pub struct LoadModelResponse {
    pub status: String,
    pub model_name: String,
}

#[derive(Clone)]
pub struct AppState {
    pub engine: Arc<ExecutionEngine>,
    pub sessions: Arc<SessionManager>,
    pub memory: Arc<MemoryStore>,
    pub tool_registry: Arc<ToolRegistry>,
}

fn json_error(msg: impl Into<String>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "error": msg.into(),
        "status": "error"
    }))
}

pub async fn handle_load_model(
    State(state): State<AppState>,
    Json(req): Json<LoadModelRequest>,
) -> Json<serde_json::Value> {
    let z1 = state.engine.z1.clone();
    let path = req.path.clone();

    let result = tokio::task::spawn_blocking(move || z1.load_model(&path)).await;

    match result {
        Ok(Ok(model_name)) => {
            info!("Z1 model loaded via Buffer Zone: {}", model_name);
            Json(serde_json::json!(LoadModelResponse {
                status: "ok".to_string(),
                model_name,
            }))
        }
        Ok(Err(e)) => {
            error!("Z1 load_model failed: {:#}", e);
            json_error(format!("{:#}", e))
        }
        Err(e) => {
            error!("load_model task panicked: {}", e);
            json_error("Internal error while loading model")
        }
    }
}

pub async fn handle_list_sessions(State(state): State<AppState>) -> Json<serde_json::Value> {
    let sessions = state.sessions.list_sessions().await;
    Json(serde_json::json!({ "sessions": sessions, "count": sessions.len() }))
}

pub async fn handle_session_detail(
    State(state): State<AppState>,
    Path(id_str): Path<String>,
) -> Json<serde_json::Value> {
    let id = match uuid::Uuid::parse_str(&id_str) {
        Ok(u) => SessionId::from(u),
        Err(_) => return json_error("Invalid session_id UUID"),
    };
    match state.sessions.get_session_detail(id).await {
        Some(summary) => Json(serde_json::json!(summary)),
        None => json_error(format!("Session not found: {id_str}")),
    }
}

pub async fn handle_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let z1 = state.engine.z1.clone();
    match z1.loaded_model_name() {
        Some(name) => Json(serde_json::json!({ "loaded": true, "model_name": name })),
        None => Json(serde_json::json!({ "loaded": false, "model_name": null })),
    }
}

pub async fn handle_models(State(state): State<AppState>) -> Json<serde_json::Value> {
    let z1 = state.engine.z1.clone();
    let models = z1.list_models();
    let default_model = z1.loaded_model_name();
    Json(serde_json::json!({ "models": models, "default": default_model }))
}

pub async fn handle_new_session(State(state): State<AppState>) -> Json<serde_json::Value> {
    let z1 = state.engine.z1.clone();
    let result = tokio::task::spawn_blocking(move || z1.new_session(None)).await;
    match result {
        Ok(Ok(())) => Json(serde_json::json!({ "status": "ok" })),
        Ok(Err(e)) => json_error(format!("{:#}", e)),
        Err(e) => {
            error!("new_session task panicked: {}", e);
            json_error("Internal error resetting session")
        }
    }
}

pub async fn handle_chat(
    State(state): State<AppState>,
    Json(req): Json<ChatRequest>,
) -> Json<serde_json::Value> {
    let session_id = if let Some(id_str) = req.session_id.clone() {
        match uuid::Uuid::parse_str(&id_str) {
            Ok(u) => SessionId::from(u),
            Err(_) => return json_error("Invalid session_id UUID"),
        }
    } else {
        let privacy = req.privacy.unwrap_or_default();
        let new_id = state
            .sessions
            .create_session(
                privacy,
                req.token_budget.unwrap_or(10_000),
                req.cost_budget.unwrap_or(1.0),
            )
            .await;
        info!("New session created: {}", new_id);
        new_id
    };

    // These must match what the engine passes to boundary.execute()
    // so the commit_reservation call inside boundary lines up correctly.
    let estimated_tokens = req.estimated_tokens.unwrap_or(500);
    let estimated_cost   = req.estimated_cost.unwrap_or(0.005);

    let engine_request = EngineRequest {
        session_id,
        message: req.message.clone(),
        capability_id: req.capability_id.clone(),
        estimated_tokens,
        estimated_cost,
        model_name: req.model.clone(),
    };

    match state
        .engine
        .process_with_tools(engine_request, &state.tool_registry)
        .await
    {
        Ok(engine_resp) => {
            // FIX: boundary.execute() inside process_with_tools already did:
            //   try_reserve_budget(estimated_tokens) → commit_reservation(estimated_tokens)
            // That is the full deduction. Do NOT call deduct_budget again.
            //
            // Only handle the delta if actual inference used MORE than estimated.
            if engine_resp.tokens_used > estimated_tokens {
                let delta_tokens = engine_resp.tokens_used - estimated_tokens;
                let delta_cost   = (engine_resp.cost_incurred - estimated_cost).max(0.0);
                if let Err(e) = state.sessions
                    .deduct_budget(session_id, delta_tokens, delta_cost)
                    .await
                {
                    warn!(
                        "Delta budget deduction failed for session {}: {}",
                        session_id, e
                    );
                }
            }

            if let Err(e) = state
                .memory
                .store_chunk(
                    &session_id.into_inner().to_string(),
                    &req.message,
                )
                .await
            {
                warn!("Failed to store memory chunk: {}", e);
            }

            Json(serde_json::json!(ChatResponse {
                session_id: engine_resp.session_id.into_inner().to_string(),
                output: engine_resp.output,
                tokens_used: engine_resp.tokens_used,
                cost_incurred: engine_resp.cost_incurred,
                status: "ok".to_string(),
            }))
        }
        Err(e) => {
            error!("Engine process failed: {}", e);
            json_error(format!("Engine error: {}", e))
        }
    }
}

pub async fn handle_create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Json<serde_json::Value> {
    let privacy = req.privacy.unwrap_or_default();
    let session_id = state
        .sessions
        .create_session(
            privacy,
            req.token_budget.unwrap_or(10_000),
            req.cost_budget.unwrap_or(1.0),
        )
        .await;
    Json(serde_json::json!(CreateSessionResponse {
        session_id: session_id.into_inner().to_string(),
        status: "ok".to_string(),
    }))
}

pub async fn handle_health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "service": "buffer-zone",
        "version": "0.1.0"
    }))
}

pub async fn handle_set_fact(
    State(state): State<AppState>,
    Json(req): Json<FactRequest>,
) -> Json<serde_json::Value> {
    match state.memory.set_fact(&req.key, &req.value).await {
        Ok(_) => Json(serde_json::json!(FactResponse {
            status: "ok".to_string(),
            message: format!("Fact '{}' stored", req.key),
        })),
        Err(e) => json_error(e.to_string()),
    }
}

async fn handle_ui() -> Response {
    match std::fs::read_to_string("z3-ui.html") {
        Ok(html) => Html(html).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "z3-ui.html not found").into_response(),
    }
}

async fn handle_stats() -> Json<serde_json::Value> {
    let cpu = std::fs::read_to_string("/proc/stat").ok()
        .and_then(|s| {
            let line = s.lines().next()?;
            let nums: Vec<u64> = line.split_whitespace().skip(1)
                .filter_map(|x| x.parse().ok()).collect();
            if nums.len() < 4 { return None; }
            let total: u64 = nums.iter().sum();
            let idle = nums[3];
            Some(if total > 0 { (total - idle) as f64 / total as f64 * 100.0 } else { 0.0 })
        }).unwrap_or(0.0);

    let (ram_used, ram_total) = std::fs::read_to_string("/proc/meminfo").ok()
        .map(|s| {
            let mut total = 0u64; let mut free = 0u64;
            let mut buffers = 0u64; let mut cached = 0u64;
            for line in s.lines() {
                let p: Vec<&str> = line.split_whitespace().collect();
                if p.len() >= 2 {
                    let v: u64 = p[1].parse().unwrap_or(0);
                    match p[0] {
                        "MemTotal:" => total = v, "MemFree:" => free = v,
                        "Buffers:" => buffers = v, "Cached:" => cached = v, _ => {}
                    }
                }
            }
            let used = total.saturating_sub(free + buffers + cached);
            (used as f64 / 1024.0 / 1024.0, total as f64 / 1024.0 / 1024.0)
        }).unwrap_or((0.0, 8.0));

    Json(serde_json::json!({
        "cpu_pct": cpu,
        "ram_used_gb": ram_used,
        "ram_total_gb": ram_total
    }))
}

async fn handle_shutdown() -> Json<serde_json::Value> {
    tokio::spawn(async {
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        std::process::exit(0);
    });
    Json(serde_json::json!({"status": "shutting_down"}))
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/chat", post(handle_chat))
        .route("/session", post(handle_create_session))
        .route("/health", get(handle_health))
        .route("/facts", post(handle_set_fact))
        .route("/load_model", post(handle_load_model))
        .route("/status", get(handle_status))
        .route("/models", get(handle_models))
        .route("/sessions", get(handle_list_sessions))
        .route("/sessions/:id", get(handle_session_detail))
        .route("/new_session", post(handle_new_session))
        .route("/", get(handle_ui))
        .route("/stats", get(handle_stats))
        .route("/shutdown", post(handle_shutdown))
        .with_state(state)
}