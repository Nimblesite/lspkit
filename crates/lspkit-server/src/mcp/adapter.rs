//! Adapter that mounts an [`lspkit::EngineApi`] implementation as an MCP server.
//!
//! The adapter does not leak `rmcp::*` types into its public surface; every
//! request/response shape is wrapped behind newtypes defined in this crate.

use std::sync::Arc;

use async_trait::async_trait;
use lspkit::EngineApi;
use serde_json::Value;

use crate::mcp::tools::{ToolDescription, ToolRegistry};

/// Result of invoking a tool.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolInvocation {
    /// Tool name.
    pub name: String,
    /// Tool-defined output, JSON-encoded.
    pub output: Value,
}

/// Errors from the adapter.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    /// No tool registered with the given name.
    #[error("tool not found: {0}")]
    UnknownTool(String),
    /// The tool's input did not match its schema.
    #[error("invalid input for {tool}: {message}")]
    InvalidInput {
        /// Tool name.
        tool: String,
        /// Detail.
        message: String,
    },
    /// The engine returned an error while servicing the tool.
    #[error("engine error: {0}")]
    Engine(String),
}

/// A tool handler: receives JSON input and produces JSON output via the engine.
#[async_trait]
pub trait ToolHandler<E: EngineApi>: Send + Sync + 'static {
    /// Invoke the tool.
    async fn invoke(&self, engine: &E, input: Value) -> Result<Value, AdapterError>;
}

/// Adapter binding an engine and its tool handlers.
#[derive(Clone)]
pub struct Adapter<E: EngineApi> {
    /// Shared engine supplied to every handler invocation.
    engine: Arc<E>,
    /// Public metadata exposed when clients enumerate tools.
    registry: ToolRegistry,
    /// Callable implementations indexed by their registered tool names.
    handlers: Vec<(String, Arc<dyn ToolHandler<E>>)>,
}

impl<E: EngineApi> Adapter<E> {
    /// New adapter wrapping `engine`.
    #[must_use]
    pub fn new(engine: Arc<E>) -> Self {
        Self {
            engine,
            registry: ToolRegistry::new(),
            handlers: Vec::new(),
        }
    }

    /// Register a tool with its description and handler.
    pub fn register(&mut self, description: ToolDescription, handler: Arc<dyn ToolHandler<E>>) {
        let name = description.name.clone();
        self.registry.register(description);
        if let Some(slot) = self.handlers.iter_mut().find(|(n, _)| n == &name) {
            slot.1 = handler;
        } else {
            self.handlers.push((name, handler));
        }
    }

    /// Snapshot registered tool descriptions.
    #[must_use]
    pub fn tools(&self) -> &[ToolDescription] {
        self.registry.list()
    }

    /// Invoke a tool by name.
    ///
    /// # Errors
    /// Returns [`AdapterError::UnknownTool`] when no handler is registered or
    /// [`AdapterError::Engine`] when the handler propagates an engine error.
    pub async fn invoke(&self, name: &str, input: Value) -> Result<ToolInvocation, AdapterError> {
        let handler = self
            .handlers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, handler)| Arc::clone(handler))
            .ok_or_else(|| AdapterError::UnknownTool(name.to_owned()))?;
        let output = handler.invoke(self.engine.as_ref(), input).await?;
        Ok(ToolInvocation {
            name: name.to_owned(),
            output,
        })
    }
}

impl<E: EngineApi> std::fmt::Debug for Adapter<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Adapter")
            .field("tools", &self.registry.list().len())
            .finish_non_exhaustive()
    }
}
