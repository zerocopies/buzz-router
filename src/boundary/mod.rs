use crate::capabilities::CapabilityRegistry;
use crate::session::SessionManager;
use crate::types::SessionId;
use anyhow::{anyhow, Result};
use std::sync::Arc;
use tracing::warn;

pub struct BoundaryEnforcer {
    registry: Arc<CapabilityRegistry>,
    sessions: Arc<SessionManager>,
}

impl BoundaryEnforcer {
    pub fn new(
        registry: Arc<CapabilityRegistry>,
        sessions: Arc<SessionManager>,
    ) -> Self {
        Self { registry, sessions }
    }

    pub async fn check(
        &self,
        session_id: SessionId,
        capability_id: &str,
        estimated_tokens: u64,
        estimated_cost: f64,
    ) -> Result<()> {
        // CHECK 1 — Capability registered?
        let capability = self
            .registry
            .get(capability_id)
            .ok_or_else(|| anyhow!("Capability not registered: {}", capability_id))?;

        // CHECK 2 — Session permits this capability?
        let privacy = self
            .sessions
            .get_privacy(session_id)
            .await
            .ok_or_else(|| anyhow!("Session not found: {}", session_id))?;

        if !self.registry.is_permitted(capability_id, privacy) {
            return Err(anyhow!(
                "Capability {} blocked by privacy level {:?}",
                capability_id,
                privacy
            ));
        }

        // CHECK 3 — Budget sufficient? (atomic deduct)
        self.sessions
            .deduct_budget(session_id, estimated_tokens, estimated_cost)
            .await?;

        // CHECK 4 — Audit log for outbound calls
        if capability.exits_buffer {
            warn!(
                "Outbound call approved: capability={} session={}",
                capability_id, session_id
            );
        }

        Ok(())
    }
}
