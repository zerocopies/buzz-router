// engine/mod.rs
//
// CHANGES FROM ORIGINAL:
// 1. ExecutionEngine now holds a Z1Engine handle (real model state,
//    same Mutex<Option<LoadedModel>> pattern already proven working
//    in z1-server.rs).
// 2. process_with_tools_inner's "thinking" step now calls real Z1
//    inference instead of the simulated echo string, when
//    capability_id == "z1_inference".
// 3. The blocking, CPU-bound Z1 call is wrapped in
//    tokio::task::spawn_blocking — required because this function
//    runs on the tokio async runtime (called from Axum), and a
//    multi-second blocking call here would otherwise freeze every
//    other request Buffer Zone is handling at the same time.
// 4. process() (the simpler, non-tool path) is left simulated as-is
//    for now — process_with_tools() is the one actually wired to
//    /chat via handle_chat in server/mod.rs, so that's the one that
//    needed to become real.
pub mod tools;

// ── BUZZ ROUTER INTELLIGENCE ───────────────────────────────────────────────
#[allow(dead_code)]
fn decide_route(prompt: &str, _ctx_size: i64) -> &'static str {
    let len = prompt.len();
    if len < 50 {
        // FIXED: Changed log::info to tracing::info
        tracing::info!("🚀 Buzz Router: Short query ({}) → FAST_MODE", len);
        "z1_fast"
    } else if prompt.contains("code") || prompt.contains("write") || len > 300 {
        // FIXED: Changed log::info to tracing::info
        tracing::info!("🧠 Buzz Router: Complex task ({}) → DEEP_MODE", len);
        "z1_deep"
    } else {
        // FIXED: Changed log::info to tracing::info
        tracing::info!("⚖️ Buzz Router: Standard query ({}) → DEFAULT", len);
        "z1_inference"
    }
}
// ── END INTELLIGENCE ───────────────────────────────────────────────────────

use crate::boundary::BoundaryEnforcer;
use crate::capabilities::CapabilityRegistry;
use crate::engine::tools::{LoopDetector, ThinkingOutput, ToolCall, ToolRegistry};
use crate::session::SessionManager;
use crate::types::{ExecutionState, SessionId};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

// ── Z1 integration ──────────────────────────────────────────────────────────
use std::path::PathBuf;
use std::sync::Mutex as StdMutex;
use z3_quantum_flow::tokenizer::Tokenizer;
use z3_quantum_flow::loader::MappedModel;
use z3_quantum_flow::graph::ForwardPass;
use z3_quantum_flow::generate::{Session as Z1Session, GenerateConfig, generate_turn_captured};
use z3_quantum_flow::gguf::GgufValue;

fn get_str_arr(metadata: &HashMap<String, GgufValue>, key: &str) -> Vec<String> {
    if let Some(GgufValue::Array(arr)) = metadata.get(key) {
        arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect()
    } else { Vec::new() }
}

fn get_f32_arr(metadata: &HashMap<String, GgufValue>, key: &str) -> Vec<f32> {
    if let Some(GgufValue::Array(arr)) = metadata.get(key) {
        arr.iter().filter_map(|v| if let GgufValue::F32(f) = v { Some(*f) } else { None }).collect()
    } else { Vec::new() }
}

fn get_u32_arr(metadata: &HashMap<String, GgufValue>, key: &str) -> Vec<u32> {
    if let Some(GgufValue::Array(arr)) = metadata.get(key) {
        arr.iter().filter_map(|v| if let GgufValue::U32(u) = v { Some(*u) } else { None }).collect()
    } else { Vec::new() }
}

const T_START_HEADER: u32 = 128_006;
const T_END_HEADER: u32 = 128_007;
const T_EOT: u32 = 128_009;
const T_NEWLINES: u32 = 271;

struct LoadedZ1Model {
    model: MappedModel,
    tokenizer: Tokenizer,
    fwd: ForwardPass,
    session: Z1Session,
    cfg: GenerateConfig,
    #[allow(dead_code)] 
    model_name: String,
}

unsafe impl Send for LoadedZ1Model {}

pub struct Z1Engine {
    models: StdMutex<HashMap<String, LoadedZ1Model>>,
    last_loaded: StdMutex<Option<String>>,
}

impl Z1Engine {
    pub fn new() -> Self {
        Self {
            models: StdMutex::new(HashMap::new()),
            last_loaded: StdMutex::new(None),
        }
    }

    pub fn load_model(&self, path: &str) -> Result<String> {
        let model_path = std::fs::canonicalize(PathBuf::from(path))
            .map_err(|_| anyhow!("Couldn't find a file at that path: {path}"))?;
        
        if model_path.extension().and_then(|e| e.to_str()) != Some("gguf") {
            return Err(anyhow!("That file doesn't look like a .gguf model file."));
        }

        let model = MappedModel::load(&model_path)
            .map_err(|_| anyhow!("Found the file, but couldn't read it as a valid model."))?;
        
        let tokens = get_str_arr(&model.header.metadata, "tokenizer.ggml.tokens");
        let scores = get_f32_arr(&model.header.metadata, "tokenizer.ggml.scores");
        let types  = get_u32_arr(&model.header.metadata, "tokenizer.ggml.token_type");
        let merges = get_str_arr(&model.header.metadata, "tokenizer.ggml.merges");
        
        let tokenizer = Tokenizer::from_gguf_parts(&tokens, &scores, &types, &merges)
            .map_err(|e| anyhow!(e.to_string()))?;
        
        let ctx_size = std::env::var("Z1_CTX_SIZE").unwrap_or("2048".into()).parse::<i64>().unwrap_or(2048);
        let fwd = ForwardPass::new(&model, ctx_size)?;
        let cfg = GenerateConfig::default();
        let session = Z1Session::new(cfg.context_len, &tokenizer, fwd.dna().arch.as_str());
        
        let model_name = model_path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "unknown model".to_string());
        
        let mut models = self.models.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        models.insert(model_name.clone(), LoadedZ1Model { model, tokenizer, fwd, session, cfg, model_name: model_name.clone() });
        drop(models);
        
        let mut last = self.last_loaded.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        *last = Some(model_name.clone());
        
        Ok(model_name)
    }

    pub fn unload_model(&self, model_name: &str) -> Result<bool> {
        let mut models = self.models.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        let removed = models.remove(model_name).is_some();
        let mut last = self.last_loaded.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        if last.as_deref() == Some(model_name) {
            *last = models.keys().next().cloned();
        }
        Ok(removed)
    }

    pub fn is_loaded(&self) -> bool {
        self.models.lock().map(|m| !m.is_empty()).unwrap_or(false)
    }

    pub fn list_models(&self) -> Vec<String> {
        self.models.lock().map(|m| m.keys().cloned().collect()).unwrap_or_default()
    }

    pub fn loaded_model_name(&self) -> Option<String> {
        self.last_loaded.lock().ok()?.clone()
    }

    pub fn run_inference(&self, prompt: &str, model_name: Option<&str>) -> Result<(String, u64)> {
        let target_name = match model_name {
            Some(n) => n.to_string(),
            None => self.loaded_model_name()
                .ok_or_else(|| anyhow!("No Z1 model is currently loaded"))?,
        };

        let mut models = self.models.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        let available: Vec<String> = models.keys().cloned().collect();
        let loaded = models.get_mut(&target_name)
            .ok_or_else(|| anyhow!("Model '{}' is not loaded. Loaded models: {:?}", target_name, available))?;
        
        if prompt.trim().is_empty() {
            return Err(anyhow!("Empty prompt"));
        }

        let (stats, text) = generate_turn_captured(
            prompt, &mut loaded.session, &mut loaded.fwd, &loaded.model, &loaded.tokenizer, &loaded.cfg)
            .map_err(|e| anyhow!("{}", e))?;

        Ok((text, stats.generated_tokens as u64))
    }

    pub fn new_session(&self, model_name: Option<&str>) -> Result<()> {
        let target_name = match model_name {
            Some(n) => n.to_string(),
            None => match self.loaded_model_name() {
                Some(n) => n,
                None => return Ok(()), 
            },
        };
        let mut models = self.models.lock().map_err(|_| anyhow!("Z1 engine lock poisoned"))?;
        if let Some(loaded) = models.get_mut(&target_name) {
            loaded.session = Z1Session::new(loaded.cfg.context_len, &loaded.tokenizer, loaded.fwd.dna().arch.as_str());
            loaded.fwd.reset_kv();
        }
        Ok(())
    }
}

// ── end Z1 integration ───────────────────────────────────────────────────────

pub struct ExecutionEngine {
    sessions: Arc<SessionManager>,
    boundary: Arc<BoundaryEnforcer>,
    _registry: Arc<CapabilityRegistry>,
    pub z1: Arc<Z1Engine>,
}

pub struct EngineRequest {
    pub session_id: SessionId,
    pub message: String,
    pub capability_id: String,
    pub estimated_tokens: u64,
    pub estimated_cost: f64,
    pub model_name: Option<String>, 
}

pub struct EngineResponse {
    pub session_id: SessionId,
    pub output: String,
    pub tokens_used: u64,
    pub cost_incurred: f64,
}

impl ExecutionEngine {
    pub fn new(
        sessions: Arc<SessionManager>,
        boundary: Arc<BoundaryEnforcer>,
        registry: Arc<CapabilityRegistry>,
    ) -> Self {
        Self { sessions, boundary, _registry: registry, z1: Arc::new(Z1Engine::new()) }
    }

    pub async fn process(&self, request: EngineRequest) -> Result<EngineResponse> {
        self.sessions
            .advance_state(request.session_id, ExecutionState::Validating)
            .await?;

        // 1. Reserve budget BEFORE doing work
        if let Err(e) = self.boundary.check_and_reserve(
            request.session_id,
            &request.capability_id,
            request.estimated_tokens,
            request.estimated_cost,
        ).await {
            error!("Boundary check failed: {}", e);
            let _ = self.sessions
                .advance_state(request.session_id, ExecutionState::Idle)
                .await;
            return Err(e.into());
        }

        // 2. Perform Work (Z1 Inference or Tools)
        // FIXED: Added 'else' branch to ensure consistent type (String, u64)
        let result = if request.capability_id == "z1_inference" {
            self.sessions.advance_state(request.session_id, ExecutionState::Thinking).await?;
            let z1 = self.z1.clone();
            let message = request.message.clone();
            let model_name = request.model_name.clone();
            
            tokio::task::spawn_blocking(move || {
                z1.run_inference(&message, model_name.as_deref())
            })
            .await
            .map_err(|e| anyhow!("Z1 task panicked: {e}"))?
        } else {
            // Fallback for other capabilities
            Ok(("Default response".to_string(), 0u64))
        };

        // 3. Commit OR Cancel based on success/failure
        match result {
            Ok((text, tokens_used)) => {
                let actual_cost = tokens_used as f64 * 0.0005; 
                
                if let Err(cancel_err) = self.boundary.commit_execution(
                    request.session_id,
                    tokens_used,
                    actual_cost
                ).await {
                    warn!("Failed to commit execution: {}. Budget may be stuck.", cancel_err);
                    let _ = self.boundary.cancel_execution(request.session_id, tokens_used, actual_cost).await;
                    return Err(anyhow!("Commit failed"));
                }
                
                return Ok(EngineResponse { 
                    session_id: request.session_id, 
                    output: text, 
                    tokens_used, 
                    cost_incurred: actual_cost 
                });
            }
            Err(e) => {
                warn!("Inference failed: {}. Releasing reserved budget.", e);
                let _ = self.boundary.cancel_execution(
                    request.session_id,
                    request.estimated_tokens,
                    request.estimated_cost
                ).await;
                return Err(e);
            }
        }
    }

    pub async fn process_with_tools(
        &self,
        request: EngineRequest,
        tool_registry: &ToolRegistry,
    ) -> Result<EngineResponse> {
        self.sessions
            .advance_state(request.session_id, ExecutionState::Validating)
            .await?;
        let result = self.process_with_tools_inner(&request, tool_registry).await;
        let _ = self.sessions
            .advance_state(request.session_id, ExecutionState::Idle)
            .await;
        result
    }

    async fn process_with_tools_inner(
        &self,
        request: &EngineRequest,
        tool_registry: &ToolRegistry,
    ) -> Result<EngineResponse> {
        if let Err(e) = self.boundary.execute(
            request.session_id,
            &request.capability_id,
            request.estimated_tokens,
            request.estimated_cost,
        ).await {
            error!("Boundary check failed: {}", e);
            return Err(e.into());
        }
        
        self.sessions
            .advance_state(request.session_id, ExecutionState::Preparing)
            .await?;
        info!("Preparing context for session {}", request.session_id);
        
        self.sessions
            .advance_state(request.session_id, ExecutionState::Routing)
            .await?;
        info!("Routing to capability: {}", request.capability_id);

        // ── REAL Z1 PATH ─────────────────────────────────────────────────
        if request.capability_id == "z1_inference" {
            self.sessions
                .advance_state(request.session_id, ExecutionState::Thinking)
                .await?;
            info!("Thinking (real Z1) — session {}", request.session_id);
            
            let z1 = self.z1.clone();
            let message = request.message.clone();
            let model_name = request.model_name.clone();
            
            // FIX: Double ?? unwraps JoinError + inner Result
            // Using a temporary variable to avoid ambiguity
            let inference_result = tokio::task::spawn_blocking(move || {
                z1.run_inference(&message, model_name.as_deref())
            })
            .await
            .map_err(|e| anyhow!("Z1 inference task panicked: {e}"))??;

            let (text, tokens_used) = inference_result;

            self.sessions
                .advance_state(request.session_id, ExecutionState::Completing)
                .await?;
            self.sessions.touch(request.session_id).await;
            
            return Ok(EngineResponse {
                session_id: request.session_id,
                output: text,
                tokens_used,
                cost_incurred: 0.0,
            });
        }
        // ── end real Z1 path ─────────────────────────────────────────────

        let mut working_memory = request.message.clone();
        let mut loop_detector = LoopDetector::new(10);
        let mut attempt: u8 = 0;
        let mut final_output: Option<String> = None;

        while attempt < 10 {
            self.sessions
                .advance_state(request.session_id, ExecutionState::Thinking)
                .await?;
            info!("Thinking iteration {}", attempt);
            
            let thinking_output = if attempt == 0 {
                ThinkingOutput::ToolCall(ToolCall {
                    tool_id: "read_file".to_string(),
                    args: HashMap::from([
                        ("path".to_string(), "input.txt".to_string())
                    ]),
                })
            } else {
                ThinkingOutput::PlainText(format!(
                    "Final answer based on: {}", working_memory
                ))
            };

            match thinking_output {
                ThinkingOutput::PlainText(text) => {
                    final_output = Some(text);
                    break;
                }
                ThinkingOutput::ToolCall(tool_call) => {
                    loop_detector.check_iteration()?;
                    loop_detector.check_repetition(&tool_call)?;
                    
                    self.sessions
                        .advance_state(
                            request.session_id,
                            ExecutionState::ToolIntercepted,
                        )
                        .await?;
                    info!("Tool intercepted: {}", tool_call.tool_id);
                    
                    if !tool_registry.contains(&tool_call.tool_id) {
                        return Err(anyhow!(
                            "Tool not permitted: {}",
                            tool_call.tool_id
                        ));
                    }

                    self.sessions
                        .advance_state(
                            request.session_id,
                            ExecutionState::ToolExecute,
                        )
                        .await?;
                    
                    let mut retry_attempt = 0;
                    loop {
                        match tool_registry.execute(&tool_call) {
                            Ok(result) => {
                                self.sessions
                                    .advance_state(
                                        request.session_id,
                                        ExecutionState::Observing,
                                    )
                                    .await?;
                                working_memory = format!(
                                    "{}\nTool {} result: {}",
                                    working_memory,
                                    result.tool_id,
                                    result.output
                                );
                                loop_detector.check_progress(&working_memory)?;
                                info!("Observed result from {}", result.tool_id);
                                break;
                            }
                            Err(e) if retry_attempt < 3 => {
                                warn!(
                                    "Tool execution failed attempt {}: {}",
                                    retry_attempt, e
                                );
                                retry_attempt += 1;
                                continue;
                            }
                            Err(e) => {
                                error!("Tool failed after 3 attempts: {}", e);
                                return Err(anyhow!(
                                    "Tool execution failed: {}", e
                                ));
                            }
                        }
                    }
                }
            }
            attempt += 1;
        }

        let output = final_output.ok_or_else(|| {
            anyhow!("ESCALATING: max iterations reached without resolution")
        })?;

        self.sessions
            .advance_state(request.session_id, ExecutionState::Completing)
            .await?;
        self.sessions.touch(request.session_id).await;

        Ok(EngineResponse {
            session_id: request.session_id,
            output,
            tokens_used: request.estimated_tokens,
            cost_incurred: request.estimated_cost,
        })
    }
}