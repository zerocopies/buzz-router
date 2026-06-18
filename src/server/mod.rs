use crate::engine::tools::ToolRegistry;
use crate::engine::{EngineRequest, ExecutionEngine};
use crate::memory::MemoryStore;
use crate::session::SessionManager;
use crate::types::{PrivacyLevel, SessionId};
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
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

    let engine_request = EngineRequest {
        session_id,
        message: req.message.clone(),
        capability_id: req.capability_id.clone(),
        estimated_tokens: req.estimated_tokens.unwrap_or(500),
        estimated_cost: req.estimated_cost.unwrap_or(0.005),
    };

    match state
        .engine
        .process_with_tools(engine_request, &state.tool_registry)
        .await
    {
        Ok(engine_resp) => {
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

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route("/chat", post(handle_chat))
        .route("/session", post(handle_create_session))
        .route("/health", get(handle_health))
        .route("/facts", post(handle_set_fact))
        .with_state(state)
}
