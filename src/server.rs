use axum::{routing::post, routing::get, Router, extract::State, Json};
use std::sync::Arc;
use std::collections::HashMap;
use tokio::net::TcpListener;

use crate::providers::{InferenceProvider, ProviderType, UserPreferences, local_z3::LocalZ3Provider, anthropic::AnthropicProvider, groq::GroqProvider, gemini::GeminiProvider};
use crate::core::decision::{self, RouteDecision};
use crate::{AppState, CloudProviders};
use crate::types::{ChatRequest, ChatResponse};

const SYSTEM_PROMPT: &str = "You are a sovereign AI assistant.";

pub async fn run_server(
    model_path: &str,
    addr: &str,
    anthropic_key: Option<&str>,
    groq_key: Option<&str>,
    gemini_key: Option<&str>,
) -> Result<(), anyhow::Error> {
    let local_provider = LocalZ3Provider::new(model_path, 2048, 512, Some(SYSTEM_PROMPT))?;

    let cloud_providers = CloudProviders {
        anthropic: anthropic_key.map(|k| Arc::new(AnthropicProvider::new(k, "claude-haiku-4-5"))),
        groq: groq_key.map(|k| Arc::new(GroqProvider::new(k, "llama-3.1-8b-instant"))),
        gemini: gemini_key.map(|k| Arc::new(GeminiProvider::new(k, "gemini-2.5-flash-lite"))),
    };

    println!("Server configuration:");
    println!("  Local: {} (zero-copy)", model_path);
    if cloud_providers.anthropic.is_some() { println!("  Cloud: Anthropic Claude enabled"); }
    if cloud_providers.groq.is_some() { println!("  Cloud: Groq enabled"); }
    if cloud_providers.gemini.is_some() { println!("  Cloud: Gemini enabled"); }
    if cloud_providers.anthropic.is_none() && cloud_providers.groq.is_none() && cloud_providers.gemini.is_none() {
        println!("  Cloud: disabled");
    }

    let state = Arc::new(AppState {
        local_provider: Arc::new(local_provider),
        cloud_providers,
    });

    let app = Router::new()
        .route("/chat", post(chat_handler))
        .route("/chat/local", post(chat_local))
        .route("/chat/groq", post(chat_groq))
        .route("/chat/gemini", post(chat_gemini))
        .route("/chat/anthropic", post(chat_anthropic))
        .route("/health", get(health_handler))
        .with_state(state);

    let listener = TcpListener::bind(addr).await?;
    println!("Server listening on {}", addr);

    axum::serve(listener, app).await?;
    Ok(())
}

/// The smart-routing entry point. `mode` on the request lets a caller
/// bypass the decision engine entirely ("local"/"groq"/"gemini"/"anthropic");
/// anything else (default: "auto") goes through decide_route().
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<ChatResponse> {
    match payload.mode.as_str() {
        "local" => return chat_local(State(state), Json(payload)).await,
        "groq" => return chat_groq(State(state), Json(payload)).await,
        "gemini" => return chat_gemini(State(state), Json(payload)).await,
        "anthropic" => return chat_anthropic(State(state), Json(payload)).await,
        _ => {}
    }

    // Provider availability for the decision engine — OpenAI intentionally
    // omitted (not wired); decide_route treats a missing key as unavailable.
    let mut available: HashMap<ProviderType, bool> = HashMap::new();
    available.insert(ProviderType::Local, true);
    available.insert(ProviderType::Anthropic, state.cloud_providers.anthropic.is_some());
    available.insert(ProviderType::Groq, state.cloud_providers.groq.is_some());
    available.insert(ProviderType::Gemini, state.cloud_providers.gemini.is_some());

    // No per-request preference fields on ChatRequest yet — uses the
    // default (speed: "cheap", force_local: false), which biases toward
    // Local for anything that isn't flagged sensitive or genuinely huge.
    let prefs = UserPreferences::default();
    let est_tokens = decision::estimate_token_count(&payload.prompt);
    let route = decision::decide_route(&payload.prompt, est_tokens, &prefs, &available);

    let (mut resp, reason) = match &route {
        RouteDecision::FullLocal { reason, .. } => {
            let r = reason.clone();
            (chat_local(State(state.clone()), Json(payload)).await, r)
        }
        RouteDecision::FullCloud { provider, reason, .. } => {
            let r = reason.clone();
            let resp = match provider {
                ProviderType::Anthropic => chat_anthropic(State(state.clone()), Json(payload)).await,
                ProviderType::Groq => chat_groq(State(state.clone()), Json(payload)).await,
                ProviderType::Gemini => chat_gemini(State(state.clone()), Json(payload)).await,
                _ => chat_local(State(state.clone()), Json(payload)).await,
            };
            (resp, r)
        }
        RouteDecision::Hybrid { gen_provider, reason, .. } => {
            let r = format!("{} (hybrid compression not yet implemented — routed full prompt to cloud)", reason);
            let resp = match gen_provider {
                ProviderType::Anthropic => chat_anthropic(State(state.clone()), Json(payload)).await,
                ProviderType::Groq => chat_groq(State(state.clone()), Json(payload)).await,
                ProviderType::Gemini => chat_gemini(State(state.clone()), Json(payload)).await,
                _ => chat_local(State(state.clone()), Json(payload)).await,
            };
            (resp, r)
        }
    };

    resp.0.warnings.insert(0, format!("auto-route: {}", reason));
    Json(resp.0)
}

async fn chat_local(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<ChatResponse> {
    let response = state.local_provider.as_ref().generate_tracked(
        &payload.prompt,
        Some(payload.max_tokens as usize),
    ).await;

    match response {
        Ok(r) => Json(to_chat_response(r)),
        Err(e) => Json(error_response(e)),
    }
}

async fn chat_groq(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<ChatResponse> {
    match &state.cloud_providers.groq {
        Some(provider) => {
            let response = provider.as_ref().generate_tracked(
                &payload.prompt,
                Some(payload.max_tokens as usize),
            ).await;
            match response {
                Ok(r) => Json(to_chat_response(r)),
                Err(e) => Json(error_response(e)),
            }
        }
        None => Json(ChatResponse {
            output: "Groq provider not configured".to_string(),
            provider: "error".to_string(),
            model_used: "none".to_string(),
            route_taken: "failed".to_string(),
            input_tokens: 0, output_tokens: 0,
            cost_incurred: 0.0, tokens_saved: 0,
            savings_vs_cloud: 0.0, processing_time_ms: 0,
            warnings: vec!["groq_not_configured".to_string()],
            stop_reason: "not_configured".to_string(),
        }),
    }
}

async fn chat_gemini(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<ChatResponse> {
    match &state.cloud_providers.gemini {
        Some(provider) => {
            let response = provider.as_ref().generate_tracked(
                &payload.prompt,
                Some(payload.max_tokens as usize),
            ).await;
            match response {
                Ok(r) => Json(to_chat_response(r)),
                Err(e) => Json(error_response(e)),
            }
        }
        None => Json(ChatResponse {
            output: "Gemini provider not configured".to_string(),
            provider: "error".to_string(),
            model_used: "none".to_string(),
            route_taken: "failed".to_string(),
            input_tokens: 0, output_tokens: 0,
            cost_incurred: 0.0, tokens_saved: 0,
            savings_vs_cloud: 0.0, processing_time_ms: 0,
            warnings: vec!["gemini_not_configured".to_string()],
            stop_reason: "not_configured".to_string(),
        }),
    }
}

async fn chat_anthropic(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ChatRequest>,
) -> Json<ChatResponse> {
    match &state.cloud_providers.anthropic {
        Some(provider) => {
            let response = provider.as_ref().generate_tracked(
                &payload.prompt,
                Some(payload.max_tokens as usize),
            ).await;
            match response {
                Ok(r) => Json(to_chat_response(r)),
                Err(e) => Json(error_response(e)),
            }
        }
        None => Json(ChatResponse {
            output: "Anthropic provider not configured".to_string(),
            provider: "error".to_string(),
            model_used: "none".to_string(),
            route_taken: "failed".to_string(),
            input_tokens: 0, output_tokens: 0,
            cost_incurred: 0.0, tokens_saved: 0,
            savings_vs_cloud: 0.0, processing_time_ms: 0,
            warnings: vec!["anthropic_not_configured".to_string()],
            stop_reason: "not_configured".to_string(),
        }),
    }
}

fn to_chat_response(r: crate::providers::ProviderResponse) -> ChatResponse {
    ChatResponse {
        output: r.output,
        provider: format!("{:?}", r.metadata.provider),
        model_used: r.metadata.model_used,
        route_taken: r.metadata.route_taken,
        input_tokens: r.metadata.input_tokens as i32,
        output_tokens: r.metadata.output_tokens as i32,
        cost_incurred: r.metadata.cost_incurred,
        tokens_saved: r.metadata.tokens_saved as i32,
        savings_vs_cloud: r.metadata.savings_vs_cloud,
        processing_time_ms: r.metadata.processing_time_ms as u128,
        warnings: r.metadata.steps,
        stop_reason: r.metadata.stop_reason,
    }
}

fn error_response(e: anyhow::Error) -> ChatResponse {
    ChatResponse {
        output: format!("Error: {}", e),
        provider: "error".to_string(),
        model_used: "none".to_string(),
        route_taken: "failed".to_string(),
        input_tokens: 0, output_tokens: 0,
        cost_incurred: 0.0, tokens_saved: 0,
        savings_vs_cloud: 0.0, processing_time_ms: 0,
        warnings: vec![e.to_string()],
        stop_reason: "error".to_string(),
    }
}

async fn health_handler() -> &'static str {
    "OK"
}
