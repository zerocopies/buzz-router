use axum::{routing::post, routing::get, Router, extract::State, Json};
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::net::TcpListener;
use crate::policy::Policy;
use crate::providers::{InferenceProvider, ProviderType, UserPreferences,
    local_z3::LocalZ3Provider, anthropic::AnthropicProvider,
    groq::GroqProvider, gemini::GeminiProvider};
use crate::{AppState, CloudProviders};
use crate::types::{ChatRequest, ChatResponse};

const SYSTEM_PROMPT: &str = "You are a sovereign AI assistant.";

pub async fn run_server(model_path: &str, addr: &str,
    anthropic_key: Option<&str>, groq_key: Option<&str>, gemini_key: Option<&str>,
) -> Result<(), anyhow::Error> {
    let policy_path = std::env::var("BUZZ_POLICY").unwrap_or_else(|_| "policy.toml".into());
    let policy = Policy::load(std::path::Path::new(&policy_path))?;
    let local_provider = LocalZ3Provider::new(model_path, 2048, 512, Some(SYSTEM_PROMPT))?;
    let cloud_providers = CloudProviders {
        anthropic: anthropic_key.map(|k| Arc::new(AnthropicProvider::new(k, "claude-haiku-4-5"))),
        groq:      groq_key.map(|k| Arc::new(GroqProvider::new(k, "llama-3.1-8b-instant"))),
        gemini:    gemini_key.map(|k| Arc::new(GeminiProvider::new(k, "gemini-2.5-flash-lite"))),
    };
    println!("Server configuration:");
    println!("  Local:  {} (zero-copy)", model_path);
    println!("  Policy: cloud_threshold={:?} | daily_budget=${:.2}",
        policy.routing.cloud_threshold, policy.cost.daily_budget_usd);
    if cloud_providers.anthropic.is_some() { println!("  Cloud:  Anthropic Claude enabled"); }
    if cloud_providers.groq.is_some()      { println!("  Cloud:  Groq enabled"); }
    if cloud_providers.gemini.is_some()    { println!("  Cloud:  Gemini enabled"); }
    if cloud_providers.anthropic.is_none() && cloud_providers.groq.is_none() && cloud_providers.gemini.is_none() {
        println!("  Cloud:  disabled");
    }
    let state = Arc::new(AppState {
        local_provider: Arc::new(local_provider),
        cloud_providers,
        policy: Arc::new(policy),
        daily_spend_microdollars: Arc::new(AtomicU64::new(0)),
        request_counter: Arc::new(AtomicU64::new(0)),
    });
    let app = Router::new()
        .route("/chat",           post(chat_handler))
        .route("/chat/local",     post(chat_local))
        .route("/chat/groq",      post(chat_groq))
        .route("/chat/gemini",    post(chat_gemini))
        .route("/chat/anthropic", post(chat_anthropic))
        .route("/health",         get(health_handler))
        .route("/stats",          get(stats_handler))
        .route("/policy",         get(policy_handler))
        .with_state(state);
    let listener = TcpListener::bind(addr).await?;
    println!("Server listening on {}", addr);
    axum::serve(listener, app).await?;
    Ok(())
}

async fn chat_handler(State(state): State<Arc<AppState>>, Json(payload): Json<ChatRequest>) -> Json<ChatResponse> {
    match payload.mode.as_str() {
        "local"     => return chat_local(State(state),     Json(payload)).await,
        "groq"      => return chat_groq(State(state),      Json(payload)).await,
        "gemini"    => return chat_gemini(State(state),    Json(payload)).await,
        "anthropic" => return chat_anthropic(State(state), Json(payload)).await,
        _           => {}
    }
    let prefs = UserPreferences::default();
    match crate::router::route_request(&payload.prompt, &*state, &prefs).await {
        Ok(r)  => Json(to_chat_response(r)),
        Err(e) => Json(error_response(e)),
    }
}

async fn chat_local(State(state): State<Arc<AppState>>, Json(payload): Json<ChatRequest>) -> Json<ChatResponse> {
    match state.local_provider.as_ref().generate_tracked(&payload.prompt, Some(payload.max_tokens as usize)).await {
        Ok(r) => Json(to_chat_response(r)), Err(e) => Json(error_response(e)),
    }
}
async fn chat_groq(State(state): State<Arc<AppState>>, Json(payload): Json<ChatRequest>) -> Json<ChatResponse> {
    match &state.cloud_providers.groq {
        Some(p) => match p.as_ref().generate_tracked(&payload.prompt, Some(payload.max_tokens as usize)).await {
            Ok(r) => Json(to_chat_response(r)), Err(e) => Json(error_response(e)),
        },
        None => Json(not_configured("groq")),
    }
}
async fn chat_gemini(State(state): State<Arc<AppState>>, Json(payload): Json<ChatRequest>) -> Json<ChatResponse> {
    match &state.cloud_providers.gemini {
        Some(p) => match p.as_ref().generate_tracked(&payload.prompt, Some(payload.max_tokens as usize)).await {
            Ok(r) => Json(to_chat_response(r)), Err(e) => Json(error_response(e)),
        },
        None => Json(not_configured("gemini")),
    }
}
async fn chat_anthropic(State(state): State<Arc<AppState>>, Json(payload): Json<ChatRequest>) -> Json<ChatResponse> {
    match &state.cloud_providers.anthropic {
        Some(p) => match p.as_ref().generate_tracked(&payload.prompt, Some(payload.max_tokens as usize)).await {
            Ok(r) => Json(to_chat_response(r)), Err(e) => Json(error_response(e)),
        },
        None => Json(not_configured("anthropic")),
    }
}

async fn health_handler() -> &'static str { "OK" }

async fn stats_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let reqs  = state.request_counter.load(Ordering::Relaxed);
    let spend = state.daily_spend_microdollars.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    let budget = state.policy.cost.daily_budget_usd;
    Json(json!({ "requests_served": reqs, "daily_spend_usd": spend,
        "daily_budget_usd": budget, "budget_remaining_usd": (budget - spend).max(0.0),
        "budget_pct_used": if budget > 0.0 { (spend / budget * 100.0).min(100.0) } else { 0.0 } }))
}

async fn policy_handler(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let p = &state.policy;
    Json(json!({ "routing": { "force_local_all": p.routing.force_local_all,
        "always_local_if_sensitive": p.routing.always_local_if_sensitive,
        "cloud_threshold": format!("{:?}", p.routing.cloud_threshold),
        "cloud_fallback_order": &p.routing.cloud_fallback_order },
        "cost": { "max_per_request_usd": p.cost.max_per_request_usd, "daily_budget_usd": p.cost.daily_budget_usd },
        "audit": { "enabled": p.audit.enabled, "log_path": &p.audit.log_path } }))
}

fn to_chat_response(r: crate::providers::ProviderResponse) -> ChatResponse {
    ChatResponse { output: r.output, provider: format!("{:?}", r.metadata.provider),
        model_used: r.metadata.model_used, route_taken: r.metadata.route_taken,
        input_tokens: r.metadata.input_tokens as i32, output_tokens: r.metadata.output_tokens as i32,
        cost_incurred: r.metadata.cost_incurred, tokens_saved: r.metadata.tokens_saved as i32,
        savings_vs_cloud: r.metadata.savings_vs_cloud,
        processing_time_ms: r.metadata.processing_time_ms as u128,
        warnings: r.metadata.steps, stop_reason: r.metadata.stop_reason }
}
fn error_response(e: anyhow::Error) -> ChatResponse {
    ChatResponse { output: format!("Error: {}", e), provider: "error".into(),
        model_used: "none".into(), route_taken: "failed".into(),
        input_tokens: 0, output_tokens: 0, cost_incurred: 0.0, tokens_saved: 0,
        savings_vs_cloud: 0.0, processing_time_ms: 0,
        warnings: vec![e.to_string()], stop_reason: "error".into() }
}
fn not_configured(name: &str) -> ChatResponse {
    ChatResponse { output: format!("{} not configured", name), provider: "error".into(),
        model_used: "none".into(), route_taken: "failed".into(),
        input_tokens: 0, output_tokens: 0, cost_incurred: 0.0, tokens_saved: 0,
        savings_vs_cloud: 0.0, processing_time_ms: 0,
        warnings: vec![format!("{}_not_configured", name)], stop_reason: "not_configured".into() }
}
