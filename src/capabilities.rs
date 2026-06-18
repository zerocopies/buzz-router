use std::collections::HashMap;
use crate::types::PrivacyLevel;

#[derive(Debug, Clone)]
pub struct Capability {
    pub id: String,
    pub exits_buffer: bool,
    pub cost_per_token: f64,
    pub context_window: usize,
    pub requires_key: bool,
    pub key_id: Option<String>,
}

#[derive(Debug)]
pub struct CapabilityRegistry {
    capabilities: HashMap<String, Capability>,
}

impl CapabilityRegistry {
    pub fn new() -> Self {
        let mut capabilities = HashMap::new();

        let mut register = |cap: Capability| {
            capabilities.insert(cap.id.clone(), cap);
        };

        register(Capability {
            id: "z1_inference".to_string(),
            exits_buffer: false,
            cost_per_token: 0.0,
            context_window: 8192,
            requires_key: false,
            key_id: None,
        });

        register(Capability {
            id: "file_system".to_string(),
            exits_buffer: false,
            cost_per_token: 0.0,
            context_window: 0,
            requires_key: false,
            key_id: None,
        });

        register(Capability {
            id: "code_executor".to_string(),
            exits_buffer: false,
            cost_per_token: 0.0,
            context_window: 0,
            requires_key: false,
            key_id: None,
        });

        register(Capability {
            id: "claude_api".to_string(),
            exits_buffer: true,
            cost_per_token: 0.003,
            context_window: 200000,
            requires_key: true,
            key_id: Some("claude_api_key".to_string()),
        });

        register(Capability {
            id: "openai_api".to_string(),
            exits_buffer: true,
            cost_per_token: 0.002,
            context_window: 128000,
            requires_key: true,
            key_id: Some("openai_api_key".to_string()),
        });

        register(Capability {
            id: "gemini_api".to_string(),
            exits_buffer: true,
            cost_per_token: 0.001,
            context_window: 1000000,
            requires_key: true,
            key_id: Some("gemini_api_key".to_string()),
        });

        register(Capability {
            id: "web_search".to_string(),
            exits_buffer: true,
            cost_per_token: 0.001,
            context_window: 0,
            requires_key: true,
            key_id: Some("search_api_key".to_string()),
        });

        Self { capabilities }
    }

    pub fn get(&self, id: &str) -> Option<&Capability> {
        self.capabilities.get(id)
    }

    pub fn is_registered(&self, id: &str) -> bool {
        self.capabilities.contains_key(id)
    }

    pub fn is_permitted(&self, id: &str, privacy: PrivacyLevel) -> bool {
        let capability = match self.get(id) {
            Some(cap) => cap,
            None => return false,
        };
        match privacy {
            PrivacyLevel::Strict => !capability.exits_buffer,
            PrivacyLevel::Public | PrivacyLevel::Private => true,
        }
    }

    pub fn list_local(&self) -> Vec<&Capability> {
        self.capabilities
            .values()
            .filter(|cap| !cap.exits_buffer)
            .collect()
    }

    pub fn list_cloud(&self) -> Vec<&Capability> {
        self.capabilities
            .values()
            .filter(|cap| cap.exits_buffer)
            .collect()
    }
}
