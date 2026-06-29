use crate::types::{ActiveSession, ExecutionState, PrivacyLevel, SessionId};
use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub tokens_remaining: u64,
    pub cost_remaining: f64,
}

#[derive(Debug)]
struct SessionEntry {
    session: ActiveSession,
    budget: Budget,
    last_active: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<SessionId, SessionEntry>>>,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn create_session(
        &self,
        privacy: PrivacyLevel,
        token_budget: u64,
        cost_budget: f64,
    ) -> SessionId {
        let session = ActiveSession::new(privacy);
        let id = session.id();
        let entry = SessionEntry {
            session,
            budget: Budget {
                tokens_remaining: token_budget,
                cost_remaining: cost_budget,
            },
            last_active: Utc::now(),
        };
        let mut lock = self.sessions.write().await;
        lock.insert(id, entry);
        id
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
        let mut lock = self.sessions.write().await;
        let entry = lock
            .get_mut(&id)
            .ok_or_else(|| anyhow!("Session not found: {}", id))?;

        if entry.budget.tokens_remaining < tokens {
            return Err(anyhow!(
                "Budget exceeded for session {}: insufficient tokens",
                id
            ));
        }
        if entry.budget.cost_remaining < cost {
            return Err(anyhow!(
                "Budget exceeded for session {}: insufficient cost balance",
                id
            ));
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
}
