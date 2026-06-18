pub mod tools;

use crate::boundary::BoundaryEnforcer;
use crate::capabilities::CapabilityRegistry;
use crate::engine::tools::{LoopDetector, ThinkingOutput, ToolCall, ToolRegistry};
use crate::session::SessionManager;
use crate::types::{ExecutionState, SessionId};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct ExecutionEngine {
    sessions: Arc<SessionManager>,
    boundary: Arc<BoundaryEnforcer>,
    _registry: Arc<CapabilityRegistry>,
}

pub struct EngineRequest {
    pub session_id: SessionId,
    pub message: String,
    pub capability_id: String,
    pub estimated_tokens: u64,
    pub estimated_cost: f64,
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
        Self { sessions, boundary, _registry: registry }
    }

    pub async fn process(&self, request: EngineRequest) -> Result<EngineResponse> {
        self.sessions
            .advance_state(request.session_id, ExecutionState::Validating)
            .await?;
        if let Err(e) = self.boundary.check(
            request.session_id,
            &request.capability_id,
            request.estimated_tokens,
            request.estimated_cost,
        ).await {
            error!("Boundary check failed: {}", e);
            let _ = self.sessions
                .advance_state(request.session_id, ExecutionState::Idle)
                .await;
            return Err(e);
        }

        self.sessions
            .advance_state(request.session_id, ExecutionState::Preparing)
            .await?;
        info!("Preparing context for session {}", request.session_id);

        self.sessions
            .advance_state(request.session_id, ExecutionState::Routing)
            .await?;
        info!("Routing to capability: {}", request.capability_id);

        self.sessions
            .advance_state(request.session_id, ExecutionState::Thinking)
            .await?;
        info!("Thinking — session {}", request.session_id);

        let output = format!(
            "[SIMULATED {} RESPONSE] Echo: {}",
            request.capability_id, request.message
        );
        self.sessions
            .advance_state(request.session_id, ExecutionState::Completing)
            .await?;
        self.sessions.touch(request.session_id).await;

        self.sessions
            .advance_state(request.session_id, ExecutionState::Idle)
            .await?;
        Ok(EngineResponse {
            session_id: request.session_id,
            output,
            tokens_used: request.estimated_tokens,
            cost_incurred: request.estimated_cost,
        })
    }

    pub async fn process_with_tools(
        &self,
        request: EngineRequest,
        tool_registry: &ToolRegistry,
    ) -> Result<EngineResponse> {
        // Advance state to Validating right at entrance
        self.sessions
            .advance_state(request.session_id, ExecutionState::Validating)
            .await?;

        // Process actual loop logic inside the inner scope
        let result = self.process_with_tools_inner(&request, tool_registry).await;

        // GUARANTEE: Infallibly force state machine recovery back to Idle on error or success
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
        if let Err(e) = self.boundary.check(
            request.session_id,
            &request.capability_id,
            request.estimated_tokens,
            request.estimated_cost,
        ).await {
            error!("Boundary check failed: {}", e);
            return Err(e);
        }

        self.sessions
            .advance_state(request.session_id, ExecutionState::Preparing)
            .await?;
        info!("Preparing context for session {}", request.session_id);

        self.sessions
            .advance_state(request.session_id, ExecutionState::Routing)
            .await?;
        info!("Routing to capability: {}", request.capability_id);

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
