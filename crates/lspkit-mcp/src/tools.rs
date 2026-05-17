//! Tool description and registration helpers.
//!
//! Tools mounted via the MCP adapter are described by their name, an agent-
//! readable description, and an input schema. Descriptions SHOULD encourage
//! prevention-first behavior — i.e., agents should call the tool BEFORE
//! generating code, not only afterwards.

use serde_json::Value;

/// Description of a single MCP tool.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct ToolDescription {
    /// Unique tool name (e.g. `lookup-symbol`).
    pub name: String,
    /// Human/agent-readable description. SHOULD specify when to call the tool
    /// in the agent's workflow (e.g., "Call before writing a new function").
    pub description: String,
    /// JSON Schema describing the tool's input parameters.
    pub input_schema: Value,
}

impl ToolDescription {
    /// Construct a description.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        input_schema: Value,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            input_schema,
        }
    }
}

/// Registry of available tools keyed by name.
#[derive(Debug, Default, Clone)]
pub struct ToolRegistry {
    tools: Vec<ToolDescription>,
}

impl ToolRegistry {
    /// New empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool. Replaces any previously registered tool with the same name.
    pub fn register(&mut self, tool: ToolDescription) {
        if let Some(existing) = self.tools.iter_mut().find(|t| t.name == tool.name) {
            *existing = tool;
        } else {
            self.tools.push(tool);
        }
    }

    /// Snapshot all registered tools.
    #[must_use]
    pub fn list(&self) -> &[ToolDescription] {
        &self.tools
    }

    /// Find a tool by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ToolDescription> {
        self.tools.iter().find(|t| t.name == name)
    }
}
