//! Server-capability builder.
//!
//! Wraps the LSP protocol's capability advertisement so consumers do not
//! depend on a specific protocol-types crate version in their public API.

use std::collections::BTreeSet;

use lspkit_vfs::PositionEncoding;

/// Decode standard initialize capabilities despite lsp-types 0.97's singular field.
///
/// LSP and VS Code send `workspace.diagnostics.refreshSupport`; the dependency's
/// Rust field currently accepts only `workspace.diagnostic`. Keep this compatibility
/// conversion at the shared protocol boundary. Implements [LSP-PULL-CAPABILITIES].
///
/// # Errors
/// Returns the original JSON deserialization error for malformed initialize data.
pub fn decode_initialize(
    mut params: serde_json::Value,
) -> Result<lsp_types::InitializeParams, serde_json::Error> {
    if let Some(workspace) = params
        .get_mut("capabilities")
        .and_then(|capabilities| capabilities.get_mut("workspace"))
        .and_then(serde_json::Value::as_object_mut)
    {
        if let Some(diagnostics) = workspace.remove("diagnostics") {
            let _ = workspace.insert("diagnostic".to_owned(), diagnostics);
        }
    }
    serde_json::from_value(params)
}

/// Kind of text-document synchronization the server supports.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSync {
    /// No synchronization.
    None,
    /// Full text on every change. Simple but expensive.
    Full,
    /// Incremental edits.
    #[default]
    Incremental,
}

/// A single advertised feature.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Feature {
    /// `textDocument/completion`.
    Completion,
    /// `textDocument/hover`.
    Hover,
    /// `textDocument/definition`.
    Definition,
    /// `textDocument/references`.
    References,
    /// Pull-style `textDocument/diagnostic`.
    DiagnosticsPull,
}

/// Builder for the server's advertised capabilities.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Capabilities {
    encoding: PositionEncoding,
    text_sync: TextSync,
    features: BTreeSet<Feature>,
}

impl Capabilities {
    /// Builder seeded with sensible defaults: incremental sync, UTF-16, no features.
    #[must_use]
    pub fn new() -> Self {
        Self {
            encoding: PositionEncoding::default(),
            text_sync: TextSync::default(),
            features: BTreeSet::new(),
        }
    }

    /// Set the negotiated position encoding.
    #[must_use]
    pub fn position_encoding(mut self, encoding: PositionEncoding) -> Self {
        self.encoding = encoding;
        self
    }

    /// Set the text-document synchronization kind.
    #[must_use]
    pub fn text_sync(mut self, kind: TextSync) -> Self {
        self.text_sync = kind;
        self
    }

    /// Advertise `feature`.
    #[must_use]
    pub fn enable(mut self, feature: Feature) -> Self {
        self.features.insert(feature);
        self
    }

    /// Whether `feature` is advertised.
    #[must_use]
    pub fn has(&self, feature: Feature) -> bool {
        self.features.contains(&feature)
    }

    /// The negotiated position encoding.
    #[must_use]
    pub fn encoding(&self) -> PositionEncoding {
        self.encoding
    }

    /// The text-sync kind.
    #[must_use]
    pub fn text_sync_kind(&self) -> TextSync {
        self.text_sync
    }

    /// Snapshot the advertised features.
    #[must_use]
    pub fn features(&self) -> Vec<Feature> {
        self.features.iter().copied().collect()
    }
}

impl Default for Capabilities {
    fn default() -> Self {
        Self::new()
    }
}
