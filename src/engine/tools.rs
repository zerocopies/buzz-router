use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub tool_id: String,
    pub args: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_id: String,
    pub output: String,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub enum ThinkingOutput {
    PlainText(String),
    ToolCall(ToolCall),
}

#[derive(Debug)]
pub struct LoopDetector {
    iteration_count: u8,
    max_iterations: u8,
    seen_hashes: HashMap<u64, u8>,
    last_working_memory_hash: Option<u64>,
    unchanged_count: u8,
}

impl LoopDetector {
    pub fn new(max_iterations: u8) -> Self {
        Self {
            iteration_count: 0,
            max_iterations,
            seen_hashes: HashMap::new(),
            last_working_memory_hash: None,
            unchanged_count: 0,
        }
    }

    pub fn check_iteration(&mut self) -> Result<()> {
        self.iteration_count += 1;
        if self.iteration_count > self.max_iterations {
            return Err(anyhow!(
                "Loop detected: max iterations ({}) exceeded",
                self.max_iterations
            ));
        }
        Ok(())
    }

    pub fn check_repetition(&mut self, tool: &ToolCall) -> Result<()> {
        let mut hasher = DefaultHasher::new();
        tool.tool_id.hash(&mut hasher);
        let mut sorted_args: Vec<_> = tool.args.iter().collect();
        sorted_args.sort_by_key(|&(k, _)| k);
        for (k, v) in sorted_args {
            k.hash(&mut hasher);
            v.hash(&mut hasher);
        }
        let hash = hasher.finish();
        let count = self.seen_hashes.entry(hash).or_insert(0);
        *count += 1;
        if *count >= 3 {
            return Err(anyhow!(
                "Loop detected: tool {} called identically {} times",
                tool.tool_id,
                count
            ));
        }
        Ok(())
    }

    pub fn check_progress(&mut self, working_memory: &str) -> Result<()> {
        let mut hasher = DefaultHasher::new();
        working_memory.hash(&mut hasher);
        let current_hash = hasher.finish();
        if Some(current_hash) == self.last_working_memory_hash {
            self.unchanged_count += 1;
            if self.unchanged_count >= 3 {
                return Err(anyhow!(
                    "Loop detected: working memory unchanged for 3 loops"
                ));
            }
        } else {
            self.unchanged_count = 0;
            self.last_working_memory_hash = Some(current_hash);
        }
        Ok(())
    }
}

pub struct ToolRegistry {
    handlers: HashMap<
        String,
        Box<dyn Fn(&ToolCall) -> Result<ToolResult> + Send + Sync>,
    >,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { handlers: HashMap::new() }
    }

    pub fn register(
        &mut self,
        tool_id: &str,
        handler: impl Fn(&ToolCall) -> Result<ToolResult> + Send + Sync + 'static,
    ) {
        self.handlers.insert(tool_id.to_string(), Box::new(handler));
    }

    /// Check existence without executing — fixes double execution bug
    pub fn contains(&self, tool_id: &str) -> bool {
        self.handlers.contains_key(tool_id)
    }

    pub fn execute(&self, tool: &ToolCall) -> Result<ToolResult> {
        let handler = self
            .handlers
            .get(&tool.tool_id)
            .ok_or_else(|| anyhow!("Unknown tool: {}", tool.tool_id))?;
        handler(tool)
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();

        registry.register("read_file", |tool| {
            let path = tool
                .args
                .get("path")
                .ok_or_else(|| anyhow!("Missing 'path' argument"))?;
            Ok(ToolResult {
                tool_id: "read_file".to_string(),
                output: format!("[SIMULATED FILE CONTENT of {}]", path),
                success: true,
            })
        });

        registry.register("write_file", |tool| {
            let path = tool
                .args
                .get("path")
                .ok_or_else(|| anyhow!("Missing 'path' argument"))?;
            let _content = tool
                .args
                .get("content")
                .ok_or_else(|| anyhow!("Missing 'content' argument"))?;
            Ok(ToolResult {
                tool_id: "write_file".to_string(),
                output: format!("[SIMULATED WRITE to {}]", path),
                success: true,
            })
        });

        registry.register("execute_code", |_tool| {
            Ok(ToolResult {
                tool_id: "execute_code".to_string(),
                output: "[SIMULATED CODE EXECUTION RESULT]".to_string(),
                success: true,
            })
        });

        registry
    }
}
