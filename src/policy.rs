use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Policy {
    pub routing: RoutingPolicy,
    pub cost:    CostPolicy,
    pub audit:   AuditPolicy,
}
impl Policy {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        if !path.exists() {
            log::warn!("[policy] {:?} not found — using defaults", path);
            return Ok(Self::default());
        }
        let text = std::fs::read_to_string(path)?;
        let p: Self = toml::from_str(&text)
            .map_err(|e| anyhow::anyhow!("[policy] parse error in {:?}: {}", path, e))?;
        log::info!("[policy] loaded from {:?} | cloud_threshold={:?} | daily_budget=${:.2}",
            path, p.routing.cloud_threshold, p.cost.daily_budget_usd);
        Ok(p)
    }
}
impl Default for Policy {
    fn default() -> Self {
        Self { routing: RoutingPolicy::default(), cost: CostPolicy::default(), audit: AuditPolicy::default() }
    }
}
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CloudThreshold { Complex, Moderate, Simple }
impl Default for CloudThreshold {
    fn default() -> Self { CloudThreshold::Complex }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RoutingPolicy {
    pub force_local_all: bool,
    pub always_local_if_sensitive: bool,
    pub cloud_threshold: CloudThreshold,
    pub cloud_fallback_order: Vec<String>,
}
impl Default for RoutingPolicy {
    fn default() -> Self {
        Self {
            force_local_all: false,
            always_local_if_sensitive: true,
            cloud_threshold: CloudThreshold::default(),
            cloud_fallback_order: vec!["groq".into(), "anthropic".into()],
        }
    }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CostPolicy {
    pub max_per_request_usd: f64,
    pub daily_budget_usd: f64,
}
impl Default for CostPolicy {
    fn default() -> Self { Self { max_per_request_usd: 0.01, daily_budget_usd: 10.0 } }
}
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AuditPolicy {
    pub enabled: bool,
    pub log_path: String,
}
impl Default for AuditPolicy {
    fn default() -> Self {
        Self { enabled: true, log_path: "/tmp/buzz-router-audit.jsonl".into() }
    }
}
