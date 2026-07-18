use crate::policy::{CloudThreshold, Policy};
use crate::providers::{ProviderType, UserPreferences};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Complexity { Simple, Moderate, Complex }

pub fn analyze_complexity(message: &str) -> Complexity {
    let lower = message.to_lowercase();
    let len = message.len();
    let complex_kw = ["analyze","compare","legal","contract","debug","architecture",
        "design","implement","write a","create a","build a","code","function",
        "class","algorithm","optimize","refactor","essay","research","report","brief","patent"];
    let simple_kw = ["hi","hello","hey","thanks","bye","yes","no",
        "what is","who is","when","where","how many"];
    if complex_kw.iter().any(|kw| lower.contains(kw)) || len > 500 { return Complexity::Complex; }
    if simple_kw.iter().any(|kw| lower.contains(kw)) && len < 100 { return Complexity::Simple; }
    Complexity::Moderate
}

pub fn is_privacy_sensitive(message: &str) -> bool {
    let lower = message.to_lowercase();
    let kw = ["ssn","social security","credit card","cvv","password","api key","secret",
        "token","confidential","proprietary","internal only","patient","diagnosis",
        "medical record","attorney","privileged","nda"];
    let has_email = lower.contains('@') && lower.contains(".com");
    let has_code = lower.contains("fn ") || lower.contains("def ")
        || lower.contains("function ") || lower.contains("import ");
    kw.iter().any(|k| lower.contains(k)) || has_email || has_code
}

#[derive(Debug, Clone)]
pub enum RouteDecision {
    FullLocal { model: String, reason: String },
    Hybrid    { compress_model: String, gen_provider: ProviderType, gen_model: String, reason: String },
    FullCloud { provider: ProviderType, model: String, reason: String },
}

fn pick_cloud(order: &[String], available: &HashMap<ProviderType, bool>) -> Option<ProviderType> {
    for name in order {
        let pt = match name.as_str() {
            "groq"      => ProviderType::Groq,
            "anthropic" => ProviderType::Anthropic,
            "gemini"    => ProviderType::Gemini,
            _           => continue,
        };
        if available.get(&pt) == Some(&true) { return Some(pt); }
    }
    None
}

pub fn decide_route(
    message: &str,
    input_tokens: usize,
    user_prefs: &UserPreferences,
    available: &HashMap<ProviderType, bool>,
    policy: &Policy,
) -> RouteDecision {
    if policy.routing.force_local_all {
        return RouteDecision::FullLocal { model: "default".into(), reason: "Policy: force_local_all=true".into() };
    }
    if user_prefs.force_local {
        return RouteDecision::FullLocal { model: "default".into(), reason: "User: force_local=true".into() };
    }
    if policy.routing.always_local_if_sensitive && is_privacy_sensitive(message) {
        return RouteDecision::FullLocal { model: "default".into(), reason: "Sensitive data detected".into() };
    }
    let complexity = analyze_complexity(message);
    let cloud_eligible = match &policy.routing.cloud_threshold {
        CloudThreshold::Complex  => complexity == Complexity::Complex,
        CloudThreshold::Moderate => complexity != Complexity::Simple,
        CloudThreshold::Simple   => true,
    };
    if !cloud_eligible {
        return RouteDecision::FullLocal {
            model: "default".into(),
            reason: format!("{:?} query below cloud_threshold ({:?})", complexity, &policy.routing.cloud_threshold),
        };
    }
    if input_tokens > 5_000 && complexity == Complexity::Complex {
        if let Some(p) = pick_cloud(&policy.routing.cloud_fallback_order, available) {
            return RouteDecision::Hybrid {
                compress_model: "local".into(), gen_model: format!("{:?}", p), gen_provider: p,
                reason: "Large input: compress locally then generate on cloud".into(),
            };
        }
    }
    if let Some(p) = pick_cloud(&policy.routing.cloud_fallback_order, available) {
        return RouteDecision::FullCloud {
            model: format!("{:?}", p), provider: p,
            reason: format!("{:?} query -> cloud", complexity),
        };
    }
    RouteDecision::FullLocal { model: "default".into(), reason: "No cloud providers available".into() }
}

pub fn estimate_token_count(text: &str) -> usize { (text.len() as f64 / 4.0) as usize }
