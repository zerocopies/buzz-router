// src/session/mod.rs
//
// Production-Ready Session Manager for zerocopy AI Inference Engine
// Features: Atomic budget reservation, TTL cleanup, audit endpoints, type-safe summaries

use crate::types::{ActiveSession, ExecutionState, PrivacyLevel, SessionId};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc, Duration as ChronoDuration};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{warn, instrument};

// ───────────────── Budget & Limits ─────────────────
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub tokens_remaining: u64,      // Actual remaining after deductions
    pub tokens_allocated: u64,      // Original allocation
    pub tokens_reserved: u64,       // Temporarily locked (pending execution)
    pub cost_remaining: f64,        // Actual remaining after deductions
    pub cost_allocated: f64,        // Original allocation
    pub cost_reserved: f64,         // Temporarily locked (pending execution)
}

impl Budget {
    pub fn new(tokens: u64, cost: f64) -> Self {
        Self {
            tokens_remaining: tokens,
            tokens_allocated: tokens,
            tokens_reserved: 0,
            cost_remaining: cost,
            cost_allocated: cost,
            cost_reserved: 0.0,
        }
    }

    /// Checks if enough budget is available AFTER accounting for reservations.
    pub fn is_available(&self, needed_tokens: u64, needed_cost: f64) -> bool {
        let available_tokens = self.tokens_remaining.saturating_sub(self.tokens_reserved);
        let available_cost = self.cost_remaining - self.cost_reserved;
        
        available_tokens >= needed_tokens && available_cost >= needed_cost
    }

    /// Mark resources as temporarily reserved.
    pub fn reserve(&mut self, tokens: u64, cost: f64) {
        self.tokens_reserved += tokens;
        self.cost_reserved += cost;
    }

    /// Commit a reservation: deduct from remaining, remove from reserved.
    pub fn commit_reservation(&mut self, tokens: u64, cost: f64) {
        self.tokens_reserved -= tokens;
        self.cost_reserved -= cost;
        self.tokens_remaining -= tokens;
        self.cost_remaining -= cost;
    }

    /// Rollback a reservation: free up the lock without deduction.
    pub fn rollback_reservation(&mut self, tokens: u64, cost: f64) {
        self.tokens_reserved -= tokens;
        self.cost_reserved -= cost;
    }

    pub fn token_utilization(&self) -> f64 {
        if self.tokens_allocated == 0 { 0.0 } else {
            let used = self.tokens_allocated - (self.tokens_remaining - self.tokens_reserved);
            used as f64 / self.tokens_allocated as f64
        }
    }

    pub fn cost_utilization(&self) -> f64 {
        if self.cost_allocated == 0.0 { 0.0 } else {
            let used = self.cost_allocated - (self.cost_remaining - self.cost_reserved);
            used / self.cost_allocated
        }
    }
}

#[derive(Debug)]
struct SessionEntry {
    session: ActiveSession,
    budget: Budget,
    created_at: DateTime<Utc>,
    last_active: DateTime<Utc>,
    #[allow(dead_code)]
    metadata: Option<HashMap<String, String>>, 
}

/// Read-only snapshot of one session's state.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub privacy: PrivacyLevel,
    pub state: ExecutionState,
    pub tokens_remaining: u64,
    pub tokens_allocated: u64,
    pub cost_remaining: f64,
    pub cost_allocated: f64,
    pub token_utilization_pct: f64,
    pub cost_utilization_pct: f64,
    pub created_at: DateTime<Utc>,
    pub last_active: DateTime<Utc>,
    pub age_seconds: f64,
}

#[derive(Debug, Default)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, SessionEntry>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    #[instrument(skip(self), level = "debug")]
    pub async fn create_session(
        &self,
        privacy: PrivacyLevel,
        token_budget: u64,
        cost_budget: f64,
    ) -> SessionId {
        let mut lock = self.sessions.write().await;
        let session = ActiveSession::new(privacy);
        let id = session.id();
        let now = Utc::now();
        
        let entry = SessionEntry {
            session,
            budget: Budget::new(token_budget, cost_budget),
            created_at: now,
            last_active: now,
            metadata: None,
        };
        lock.insert(id, entry);
        id
    }

    pub async fn get_budget(&self, id: SessionId) -> Option<Budget> {
        let lock = self.sessions.read().await;
        lock.get(&id).map(|e| e.budget)
    }

    pub async fn get_privacy(&self, id: SessionId) -> Option<PrivacyLevel> {
        let lock = self.sessions.read().await;
        lock.get(&id).map(|e| e.session.privacy_level())
    }

    pub async fn get_state(&self, id: SessionId) -> Option<ExecutionState> {
        let lock = self.sessions.read().await;
        lock.get(&id).map(|e| e.session.state())
    }

    pub async fn advance_state(
        &self,
        id: SessionId,
        next: ExecutionState,
    ) -> Result<()> {
        let mut lock = self.sessions.write().await;
        let entry = lock
            .get_mut(&id)
            .ok_or_else(|| anyhow!("Session not found: {}", id))?;
        entry.session.advance_state(next)?;
        entry.last_active = Utc::now();
        Ok(())
    }

    pub async fn deduct_budget(
        &self,
        id: SessionId,
        tokens: u64,
        cost: f64,
    ) -> Result<()> {
        // Legacy direct deduction (for non-reserved flows)
        let mut lock = self.sessions.write().await;
        let entry = lock.get_mut(&id).ok_or_else(|| anyhow!("Session not found: {}", id))?;

        if entry.budget.tokens_remaining < tokens {
            return Err(anyhow!("Budget exceeded: insufficient tokens"));
        }
        if entry.budget.cost_remaining < cost {
            return Err(anyhow!("Budget exceeded: insufficient cost"));
        }
        
        entry.budget.tokens_remaining -= tokens;
        entry.budget.cost_remaining -= cost;
        entry.last_active = Utc::now();
        Ok(())
    }

    pub async fn terminate_session(&self, id: SessionId) -> Result<()> {
        let mut lock = self.sessions.write().await;
        if lock.remove(&id).is_none() {
            return Err(anyhow!("Session not found: {}", id));
        }
        Ok(())
    }

    pub async fn touch(&self, id: SessionId) {
        let mut lock = self.sessions.write().await;
        if let Some(entry) = lock.get_mut(&id) {
            entry.last_active = Utc::now();
        }
    }

    // ── NEW: Audit/Visibility methods ────────────────────────────────────
    pub async fn list_sessions(&self) -> Vec<SessionSummary> {
        let lock = self.sessions.read().await;
        let now = Utc::now();
        lock.values()
            .map(|entry| build_summary(entry, now))
            .collect()
    }

    pub async fn get_session_detail(&self, id: SessionId) -> Option<SessionSummary> {
        let lock = self.sessions.read().await;
        lock.get(&id).map(|entry| build_summary(entry, Utc::now()))
    }

    // ── NEW: ATOMIC RESERVATION LOGIC (The Fix) ──────────────────────────

    /// Atomically checks availability AND reserves budget.
    /// Prevents race conditions where two requests pass check but fail on write.
    pub async fn try_reserve_budget(
        &self,
        id: SessionId,
        tokens: u64,
        cost: f64,
    ) -> Result<()> {
        let mut lock = self.sessions.write().await;
        let entry = lock.get_mut(&id).ok_or_else(|| anyhow!("Session not found: {}", id))?;

        if !entry.budget.is_available(tokens, cost) {
            return Err(anyhow!(
                "Insufficient available budget for session {}: 
                 Needed {} tokens / {:.4} cost, 
                 Available: {} tokens / {:.4} cost",
                id,
                tokens,
                cost,
                entry.budget.tokens_remaining.saturating_sub(entry.budget.tokens_reserved),
                entry.budget.cost_remaining - entry.budget.cost_reserved
            ));
        }

        entry.budget.reserve(tokens, cost);
        entry.last_active = Utc::now();
        Ok(())
    }

    /// Commits a previous reservation, turning it into an actual deduction.
    pub async fn commit_reservation(
        &self,
        id: SessionId,
        tokens: u64,
        cost: f64,
    ) -> Result<()> {
        let mut lock = self.sessions.write().await;
        let entry = lock.get_mut(&id).ok_or_else(|| anyhow!("Session not found: {}", id))?;

        if entry.budget.tokens_reserved < tokens || entry.budget.cost_reserved < cost {
            warn!("Commit mismatch: Session {} tried to commit {}t/{}c but has {}t/{}c reserved.",
                  id, tokens, cost, entry.budget.tokens_reserved, entry.budget.cost_reserved);
            entry.budget.rollback_reservation(tokens.min(entry.budget.tokens_reserved), cost.min(entry.budget.cost_reserved));
            return Err(anyhow!("Reservation mismatch"));
        }

        entry.budget.commit_reservation(tokens, cost);
        entry.last_active = Utc::now();
        Ok(())
    }

    /// Cancels a reservation without deducting (release lock).
    pub async fn cancel_reservation(
        &self,
        id: SessionId,
        tokens: u64,
        cost: f64,
    ) -> Result<()> {
        let mut lock = self.sessions.write().await;
        let entry = lock.get_mut(&id).ok_or_else(|| anyhow!("Session not found: {}", id))?;
        
        entry.budget.rollback_reservation(tokens, cost);
        entry.last_active = Utc::now();
        Ok(())
    }

    // Cleanup helpers
    pub async fn cleanup_idle(&self, max_age_secs: i64) -> usize {
        let mut lock = self.sessions.write().await;
        let cutoff = Utc::now() - ChronoDuration::seconds(max_age_secs);
        let before = lock.len();
        lock.retain(|_, e| e.last_active > cutoff || false);
        before - lock.len()
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }
}

fn build_summary(entry: &SessionEntry, now: DateTime<Utc>) -> SessionSummary {
    SessionSummary {
        session_id: entry.session.id().to_string(), // Convert to string for serialization
        privacy: entry.session.privacy_level(),
        state: entry.session.state(),
        tokens_remaining: entry.budget.tokens_remaining,
        tokens_allocated: entry.budget.tokens_allocated,
        cost_remaining: entry.budget.cost_remaining,
        cost_allocated: entry.budget.cost_allocated,
        token_utilization_pct: entry.budget.token_utilization(),
        cost_utilization_pct: entry.budget.cost_utilization(),
        created_at: entry.created_at,
        last_active: entry.last_active,
        age_seconds: (now - entry.created_at).num_seconds() as f64,
    }
}
