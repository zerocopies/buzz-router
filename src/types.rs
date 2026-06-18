use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(Uuid);

impl SessionId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    pub fn into_inner(self) -> Uuid {
        self.0
    }
}

impl From<Uuid> for SessionId {
    fn from(uuid: Uuid) -> Self {
        Self(uuid)
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PrivacyLevel {
    Public,
    #[default]
    Private,
    Strict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryTier {
    Working,
    Session,
    LongTerm,
    KeyFacts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionState {
    Idle,
    Validating,
    Preparing,
    Routing,
    Thinking,
    ToolIntercepted,
    ToolExecute,
    Observing,
    Completing,
}

impl ExecutionState {
    pub fn transition(self, next: ExecutionState) -> Result<Self> {
        let valid = match (self, next) {
            (Self::Idle, Self::Validating) => true,
            (Self::Validating, Self::Preparing) => true,
            (Self::Validating, Self::Idle) => true,
            (Self::Preparing, Self::Routing) => true,
            (Self::Routing, Self::Thinking) => true,
            (Self::Thinking, Self::ToolIntercepted) => true,
            (Self::Thinking, Self::Completing) => true,
            (Self::ToolIntercepted, Self::ToolExecute) => true,
            (Self::ToolIntercepted, Self::Idle) => true,
            (Self::ToolExecute, Self::Observing) => true,
            (Self::Observing, Self::Thinking) => true,
            (Self::Observing, Self::Completing) => true,
            (Self::Completing, Self::Idle) => true,
            _ => false,
        };
        if valid {
            Ok(next)
        } else {
            Err(anyhow!(
                "Invalid state transition from {:?} to {:?}",
                self,
                next
            ))
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: SessionId,
    pub privacy_level: PrivacyLevel,
    pub created_at: DateTime<Utc>,
}

impl SessionMetadata {
    pub fn new(privacy_level: PrivacyLevel) -> Self {
        Self {
            id: SessionId::new(),
            privacy_level,
            created_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ActiveSession {
    pub metadata: SessionMetadata,
    pub current_state: ExecutionState,
}

impl ActiveSession {
    pub fn new(privacy_level: PrivacyLevel) -> Self {
        Self {
            metadata: SessionMetadata::new(privacy_level),
            current_state: ExecutionState::Idle,
        }
    }

    pub fn id(&self) -> SessionId {
        self.metadata.id
    }

    pub fn privacy_level(&self) -> PrivacyLevel {
        self.metadata.privacy_level
    }

    pub fn state(&self) -> ExecutionState {
        self.current_state
    }

    pub fn advance_state(&mut self, next: ExecutionState) -> Result<()> {
        self.current_state = self.current_state.transition(next)?;
        Ok(())
    }
}
