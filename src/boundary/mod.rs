// src/boundary/enforcer.rs
//
// High-Performance Boundary Enforcer for zerocopy AI Inference Engine
// Uses atomic reservation to prevent race conditions and ensure budget integrity.

use crate::capabilities::CapabilityRegistry;
use crate::session::SessionManager;
use crate::types::{PrivacyLevel, SessionId};
use anyhow::Result;
use std::sync::Arc;
use tracing::{instrument, Level, warn, info_span};

/// Granular error types for client-side handling
#[derive(Debug, Clone, serde::Serialize)]
pub enum EnforcementError {
    CapabilityNotFound(String),
    PrivacyViolation { capability: String, required_privacy: Option<PrivacyLevel> },
    SessionNotFound(SessionId),
    BudgetExhausted {
        session_id: SessionId,
        type_: &'static str, // "token" or "cost"
        needed: f64,
        available: f64,
    },
    ReservationFailed { reason: String },
}

impl std::fmt::Display for EnforcementError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CapabilityNotFound(c) => write!(f, "Capability '{}' not registered", c),
            Self::PrivacyViolation { capability, required_privacy } => 
                write!(f, "Capability '{}' blocked by privacy level {:?}", capability, required_privacy),
            Self::SessionNotFound(id) => write!(f, "Session {} not found", id),
            Self::BudgetExhausted { session_id, type_, needed, available } =>
                write!(f, "Session {}: Insufficient {} (need {}, have {})", session_id, type_, needed, available),
            Self::ReservationFailed { reason } => write!(f, "Reservation failed: {}", reason),
        }
    }
}

impl std::error::Error for EnforcementError {}

pub struct BoundaryEnforcer {
    registry: Arc<CapabilityRegistry>,
    sessions: Arc<SessionManager>,
    log_outbound: bool,
}

impl BoundaryEnforcer {
    pub fn new(registry: Arc<CapabilityRegistry>, sessions: Arc<SessionManager>) -> Self {
        Self {
            registry,
            sessions,
            log_outbound: true,
        }
    }

    /// Performs checks and atomically reserves budget.
    /// Returns Ok(()) if reservation is successful.
    /// Caller must later call `commit_execution` or `cancel_execution`.
    #[instrument(skip(self), err(level = Level::WARN))]
    pub async fn check_and_reserve(
        &self,
        session_id: SessionId,
        capability_id: &str,
        estimated_tokens: u64,
        estimated_cost: f64,
    ) -> Result<(), EnforcementError> {
        let span = info_span!("boundary_check", session_id = %session_id, capability = capability_id);
        let _guard = span.enter();

        // 1. Check Capability Existence
        let capability = self.registry.get(capability_id)
            .ok_or_else(|| EnforcementError::CapabilityNotFound(capability_id.to_string()))?;

        // 2. Check Privacy Compatibility
        let privacy = self.sessions.get_privacy(session_id).await
            .ok_or(EnforcementError::SessionNotFound(session_id))?;

        if !self.registry.is_permitted(capability_id, privacy) {
            warn!("Privacy violation: {} vs {:?}", capability_id, privacy);
            return Err(EnforcementError::PrivacyViolation {
                capability: capability_id.to_string(),
                required_privacy: None,
            });
        }

        // 3. Atomic Budget Check & Reservation
        // This prevents race conditions: if this fails, no budget is lost.
        if let Err(e) = self.sessions.try_reserve_budget(session_id, estimated_tokens, estimated_cost).await {
            return Err(EnforcementError::ReservationFailed { reason: e.to_string() });
        }

        // 4. Audit Log for Outbound Calls
        if capability.exits_buffer && self.log_outbound {
            warn!(
                "Outbound reservation approved: capability={} session={}",
                capability_id, session_id
            );
        }

        Ok(())
    }

    /// Commits the reservation after successful execution.
    pub async fn commit_execution(
        &self,
        session_id: SessionId,
        actual_tokens: u64,
        actual_cost: f64,
    ) -> Result<(), EnforcementError> {
        self.sessions
            .commit_reservation(session_id, actual_tokens, actual_cost)
            .await
            .map_err(|e| EnforcementError::ReservationFailed { reason: e.to_string() })
    }

    /// Cancels the reservation if execution fails.
    pub async fn cancel_execution(
        &self,
        session_id: SessionId,
        reserved_tokens: u64,
        reserved_cost: f64,
    ) -> Result<(), EnforcementError> {
        self.sessions
            .cancel_reservation(session_id, reserved_tokens, reserved_cost)
            .await
            .map_err(|e| EnforcementError::ReservationFailed { reason: e.to_string() })
    }
    
    /// Convenience method for simple synchronous workflows.
    pub async fn execute(
        &self,
        session_id: SessionId,
        capability_id: &str,
        estimated_tokens: u64,
        estimated_cost: f64,
    ) -> Result<(), EnforcementError> {
        // Reserve
        self.check_and_reserve(session_id, capability_id, estimated_tokens, estimated_cost).await?;
        
        // TODO: Insert actual inference work here
        
        // Commit
        self.commit_execution(session_id, estimated_tokens, estimated_cost).await?;
        
        if let Some(cap) = self.registry.get(capability_id) {
            if cap.exits_buffer && self.log_outbound {
                warn!("Outbound execution finalized: {}", capability_id);
            }
        }
        
        Ok(())
    }
}
