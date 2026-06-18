use buffer_zone::boundary::BoundaryEnforcer;
use buffer_zone::capabilities::CapabilityRegistry;
use buffer_zone::engine::tools::ToolRegistry;
use buffer_zone::engine::ExecutionEngine;
use buffer_zone::memory::MemoryStore;
use buffer_zone::server::{create_router, AppState};
use buffer_zone::session::SessionManager;

use std::path::Path;
use std::sync::Arc;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    info!("Buffer Zone starting...");

    // a. Memory store
    let memory = MemoryStore::new(
        Path::new("buffer_zone_memory.db")
    ).await?;

    // b. Session manager
    let sessions = SessionManager::new();
    let sessions_arc = Arc::new(sessions);

    // c. Capability registry — wrap in Arc before dual use
    let registry_arc = Arc::new(CapabilityRegistry::new());

    // d. Boundary enforcer
    let boundary_arc = Arc::new(BoundaryEnforcer::new(
        registry_arc.clone(),
        sessions_arc.clone(),
    ));

    // e. Execution engine
    let engine_arc = Arc::new(ExecutionEngine::new(
        sessions_arc.clone(),
        boundary_arc,
        registry_arc,
    ));

    // f. Tool registry
    let tool_registry_arc = Arc::new(ToolRegistry::with_defaults());

    let state = AppState {
        engine: engine_arc,
        sessions: sessions_arc,
        memory: Arc::new(memory),
        tool_registry: tool_registry_arc,
    };

    let router = create_router(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:7474").await?;
    info!("Buffer Zone listening on 127.0.0.1:7474");
    axum::serve(listener, router).await?;

    Ok(())
}
