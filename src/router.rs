use crate::AppState;
use crate::core::{cost, privacy};
use crate::core::decision::{self, RouteDecision};
use crate::providers::{InferenceProvider, ProviderResponse, ProviderType, UserPreferences};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::time::Instant;

pub async fn route_request(
    message: &str, state: &AppState, user_prefs: &UserPreferences,
) -> Result<ProviderResponse, anyhow::Error> {
    let req_id = state.request_counter.fetch_add(1, Ordering::Relaxed);
    let _start = Instant::now();
    let input_tokens = decision::estimate_token_count(message);
    let privacy_scan = privacy::scan_text(message);
    let mut available: HashMap<ProviderType, bool> = HashMap::new();
    available.insert(ProviderType::Local, true);
    available.insert(ProviderType::Anthropic, state.cloud_providers.anthropic.is_some());
    available.insert(ProviderType::Groq,      state.cloud_providers.groq.is_some());
    available.insert(ProviderType::Gemini,    state.cloud_providers.gemini.is_some());
    let mut route = decision::decide_route(message, input_tokens, user_prefs, &available, &state.policy);
    if matches!(&route, RouteDecision::FullCloud { .. } | RouteDecision::Hybrid { .. }) {
        let est = cost::calculate_cost(ProviderType::Groq, input_tokens, 100);
        let spent = state.daily_spend_microdollars.load(Ordering::Relaxed) as f64 / 1_000_000.0;
        if est > state.policy.cost.max_per_request_usd {
            log::warn!("[req={}] Cost cap: est ${:.6} > max ${:.6} — local", req_id, est, state.policy.cost.max_per_request_usd);
            route = RouteDecision::FullLocal { model: "default".into(),
                reason: format!("Cost cap: est ${:.6} exceeds per-request max", est) };
        } else if spent >= state.policy.cost.daily_budget_usd {
            log::warn!("[req={}] Daily budget ${:.4} exhausted — local", req_id, state.policy.cost.daily_budget_usd);
            route = RouteDecision::FullLocal { model: "default".into(),
                reason: format!("Cost cap: daily budget ${:.4} exhausted", state.policy.cost.daily_budget_usd) };
        }
    }
    log::info!("[req={}] {:?} | {}t | sensitive={}", req_id, route, input_tokens, privacy_scan.is_sensitive);
    let mut resp = match &route {
        RouteDecision::FullLocal { reason, .. } => {
            let mut r = state.local_provider.generate_tracked(message, None).await?;
            r.metadata.steps.insert(0, format!("auto-route: {}", reason));
            r
        }
        RouteDecision::FullCloud { provider, reason, .. } => {
            match cloud_dispatch(message, state, provider).await {
                Ok(mut r) => {
                    let micro = (r.metadata.cost_incurred * 1_000_000.0) as u64;
                    state.daily_spend_microdollars.fetch_add(micro, Ordering::Relaxed);
                    r.metadata.steps.insert(0, format!("auto-route: {}", reason));
                    r
                }
                Err(e) => {
                    log::warn!("[req={}] Cloud failed ({}) — fallback local", req_id, e);
                    let mut r = state.local_provider.generate_tracked(message, None).await?;
                    r.metadata.steps.insert(0, format!("auto-route: cloud failed ({}) — fallback local", e));
                    r
                }
            }
        }
        RouteDecision::Hybrid { gen_provider, reason, .. } => {
            match hybrid_dispatch(message, state, gen_provider, reason, req_id).await {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[req={}] Hybrid failed ({}) — fallback local", req_id, e);
                    let mut r = state.local_provider.generate_tracked(message, None).await?;
                    r.metadata.steps.insert(0, format!("auto-route: hybrid failed ({}) — fallback local", e));
                    r
                }
            }
        }
    };
    if state.policy.audit.enabled {
        write_audit(req_id, &resp, privacy_scan.is_sensitive, &state.policy.audit.log_path);
    }
    Ok(resp)
}

async fn cloud_dispatch(message: &str, state: &AppState, provider: &ProviderType)
    -> Result<ProviderResponse, anyhow::Error> {
    match provider {
        ProviderType::Anthropic => state.cloud_providers.anthropic.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Anthropic not configured"))?
            .generate_tracked(message, None).await,
        ProviderType::Groq => state.cloud_providers.groq.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Groq not configured"))?
            .generate_tracked(message, None).await,
        ProviderType::Gemini => state.cloud_providers.gemini.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Gemini not configured"))?
            .generate_tracked(message, None).await,
        _ => Err(anyhow::anyhow!("Unknown provider: {:?}", provider)),
    }
}

async fn hybrid_dispatch(message: &str, state: &AppState, gen_provider: &ProviderType, reason: &str, req_id: u64)
    -> Result<ProviderResponse, anyhow::Error> {
    let cp = format!("Summarize in 2-3 sentences:
{}

Output only the summary.", message);
    let comp = state.local_provider.generate_tracked(&cp, Some(100)).await?;
    let ctok = decision::estimate_token_count(&comp.output);
    log::info!("[req={}] Hybrid: {} -> {} tokens (local compress)", req_id, decision::estimate_token_count(message), ctok);
    let cloud_prompt = format!("Context:
{}

Question: {}", comp.output, message);
    let mut cr = cloud_dispatch(&cloud_prompt, state, gen_provider).await?;
    state.daily_spend_microdollars.fetch_add((cr.metadata.cost_incurred * 1_000_000.0) as u64, Ordering::Relaxed);
    cr.metadata.steps.insert(0, format!("auto-route: {}", reason));
    cr.metadata.steps.insert(1, format!("hybrid: {} -> {} tokens compressed locally", decision::estimate_token_count(message), ctok));
    Ok(cr)
}

fn write_audit(req_id: u64, resp: &ProviderResponse, sensitive: bool, log_path: &str) {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_secs();
    let record = serde_json::json!({
        "ts": ts,
        "req": req_id,
        "provider": format!("{:?}", resp.metadata.provider),
        "route": resp.metadata.route_taken,
        "sensitive": sensitive,
        "in_tok": resp.metadata.input_tokens,
        "out_tok": resp.metadata.output_tokens,
        "cost_usd": resp.metadata.cost_incurred,
        "stop": resp.metadata.stop_reason,
    });
    let mut line = record.to_string();
    line.push('\n');
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(log_path) {
        let _ = f.write_all(line.as_bytes());
    }
}
