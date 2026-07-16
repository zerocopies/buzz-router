use async_trait::async_trait;
use std::sync::Mutex;
use qfz3::Engine;
use qfz3::engine::StopReason;
use crate::providers::{InferenceProvider, ProviderType, ProviderResponse, RouteMetadata};

pub struct LocalZ3Provider {
    engine: Mutex<Engine>,
    model_name: String,
    max_tokens: usize,
}

// SAFETY: Engine contains raw ggml pointers. All access is Mutex-guarded.
unsafe impl Send for LocalZ3Provider {}
unsafe impl Sync for LocalZ3Provider {}

impl LocalZ3Provider {
    pub fn new(
        model_path: &str,
        context_len: usize,
        max_tokens: usize,
        _system_prompt: Option<&str>,
    ) -> Result<Self, anyhow::Error> {
        let engine = Engine::load(model_path, context_len, _system_prompt).map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(Self {
            engine: Mutex::new(engine),
            model_name: "qfz3-local".to_string(),
            max_tokens,
        })
    }
}

#[async_trait]
impl InferenceProvider for LocalZ3Provider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Local
    }

    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn input_cost_per_mtok(&self) -> f64 {
        0.0
    }

    fn output_cost_per_mtok(&self) -> f64 {
        0.0
    }

    fn is_local(&self) -> bool {
        true
    }

    async fn generate(&self, prompt: &str, max_tokens: Option<usize>) -> Result<String, anyhow::Error> {
        let tokens = max_tokens.unwrap_or(self.max_tokens) as i32;
        let prompt_owned = prompt.to_string();
        let mut engine = self.engine.lock().map_err(|e| anyhow::anyhow!("Engine lock poisoned: {}", e))?;
        // Reset per request — see generate_tracked for why.
        engine.reset();
        let (output, _tok_count) = engine.generate_sync(&prompt_owned.as_str(), tokens).map_err(|e| anyhow::anyhow!("{}", e))?;
        Ok(output)
    }

    async fn generate_tracked(&self, prompt: &str, max_tokens: Option<usize>) -> Result<ProviderResponse, anyhow::Error> {
        let max_tok = max_tokens.unwrap_or(self.max_tokens) as i32;
        let prompt_owned = prompt.to_string();

        let mut engine = self.engine.lock().map_err(|e| anyhow::anyhow!("Engine lock poisoned: {}", e))?;
        // Reset KV cache + turn counter so every HTTP request is an
        // independent conversation. Without this, request N's tokens get
        // silently appended to request N-1's KV state via
        // build_followup_chat_tokens, producing answers that respond to
        // the WRONG prompt — confirmed live: a fresh "moons of Jupiter"
        // query answered as a continuation of a prior, unrelated request.
        engine.reset();
        let out = engine.generate_rich(&prompt_owned, max_tok).map_err(|e| anyhow::anyhow!("{}", e))?;
        drop(engine);

        // Real tokenizer counts from the engine (prompt_tokens/completion_tokens),
        // not the char_len/4 heuristic this used to call via estimate_tokens().
        let stop_reason = match out.stop_reason {
            StopReason::EndOfTurn => "end_turn",
            StopReason::MaxTokens => "max_tokens",
        }.to_string();
        let processing_time_ms = (out.prompt_ms + out.generate_ms).round() as u64;

        Ok(ProviderResponse {
            output: out.text,
            metadata: RouteMetadata {
                provider: self.provider_type(),
                model_used: self.model_name().to_string(),
                route_taken: "local_direct".to_string(),
                input_tokens: out.prompt_tokens,
                output_tokens: out.completion_tokens,
                cost_incurred: 0.0,
                tokens_saved: 0,
                savings_vs_cloud: 100.0,
                processing_time_ms,
                steps: vec!["LocalZ3 inference".to_string()],
                stop_reason,
            },
        })
    }
}
