use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use super::{InferenceProvider, ProviderType, ProviderResponse, RouteMetadata};

#[derive(Debug, Deserialize, Default)]
struct GroqResponse {
    #[serde(default)]
    choices: Vec<GroqChoice>,
    #[serde(default)]
    usage: GroqUsage,
}

#[derive(Debug, Deserialize, Default)]
struct GroqChoice {
    #[serde(default)]
    message: GroqMessage,
}

#[derive(Debug, Deserialize, Clone, Default)]
struct GroqMessage {
    #[serde(default)]
    content: String,
}

#[derive(Debug, Deserialize, Default)]
struct GroqUsage {
    #[serde(default)]
    total_tokens: usize,
    #[serde(default)]
    prompt_tokens: usize,
    #[serde(default)]
    completion_tokens: usize,
}

pub struct GroqProvider {
    client: Client,
    api_key: String,
    model: String,
}

impl GroqProvider {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            client: Client::new(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }
}

#[async_trait]
impl InferenceProvider for GroqProvider {
    fn provider_type(&self) -> ProviderType {
        ProviderType::Groq
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn input_cost_per_mtok(&self) -> f64 {
        0.10 // $0.10 / MTok (approximate)
    }

    fn output_cost_per_mtok(&self) -> f64 {
        0.10
    }

    fn is_local(&self) -> bool {
        false
    }

    async fn generate(&self, prompt: &str, max_tokens: Option<usize>) -> Result<String, anyhow::Error> {
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens.unwrap_or(100),
            "temperature": 0.7
        });

        let client = Client::new();
        let resp = client.post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let raw = resp.text().await?;
        eprintln!("Raw Groq response: {}", raw);
        println!("{:?} {:?}", resp.status(), resp.headers());
        let raw = resp.text().await?;
        eprintln!("Raw Groq response: {}", raw);
        println!("{:?} {:?}", resp.status(), resp.headers());
        let parsed: GroqResponse = resp.json().await?;
        
        if parsed.choices.is_empty() {
            return Err(anyhow::anyhow!("No choices in response"));
        }
        
        Ok(parsed.choices[0].message.content.clone())
    }

    async fn generate_tracked(&self, prompt: &str, max_tokens: Option<usize>) -> Result<ProviderResponse, anyhow::Error> {
        use std::time::Instant;

        let start = Instant::now();
        
        let payload = serde_json::json!({
            "model": self.model,
            "messages": [{"role": "user", "content": prompt}],
            "max_tokens": max_tokens.unwrap_or(100),
            "temperature": 0.7
        });

        let client = Client::new();
        let resp = client.post("https://api.groq.com/openai/v1/chat/completions")
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await?;

        let raw = resp.text().await?;
        eprintln!("Raw Groq response: {}", raw);
        println!("{:?} {:?}", resp.status(), resp.headers());
        let raw = resp.text().await?;
        eprintln!("Raw Groq response: {}", raw);
        println!("{:?} {:?}", resp.status(), resp.headers());
        let parsed: GroqResponse = resp.json().await?;
        let elapsed = start.elapsed().as_millis() as u64;

        if parsed.choices.is_empty() {
            return Err(anyhow::anyhow!("No choices in response"));
        }

        let output = parsed.choices[0].message.content.clone();
        let input_tokens = parsed.usage.prompt_tokens;
        let output_tokens = parsed.usage.completion_tokens;
        let cost = (input_tokens as f64 + output_tokens as f64) * self.input_cost_per_mtok() / 1_000_000.0;

        Ok(ProviderResponse {
            output,
            metadata: RouteMetadata {
                provider: self.provider_type(),
                model_used: self.model_name().to_string(),
                route_taken: "cloud_groq".to_string(),
                input_tokens,
                output_tokens,
                cost_incurred: cost,
                tokens_saved: 0,
                savings_vs_cloud: 0.0,
                processing_time_ms: elapsed,
                steps: vec!["Groq API call".to_string()],
            },
        })
    }
}
