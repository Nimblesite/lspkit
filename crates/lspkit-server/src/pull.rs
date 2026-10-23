//! Incremental pull-diagnostic truth. Implements [LSP-PULL-SALSA] and
//! [LSP-PULL-CONCURRENCY].
//!
//! Only Salsa retains computed diagnostics. The store retains input handles and
//! a lock-free revision counter; protocol-side invalidation never waits behind
//! a running compiler query.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use lsp_types::Diagnostic;
use salsa::Setter;

/// A backend failure must remain an error, never an empty clean report.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("diagnostic analysis unavailable: {0}")]
pub struct AnalysisError(pub String);

/// Diagnostics for one current compiler snapshot, or an honest failure.
pub type Analysis = Result<Arc<Vec<Diagnostic>>, AnalysisError>;

/// The compiler callback, invoked only on a Salsa miss.
pub type Analyzer = Arc<dyn Fn(&str) -> Analysis + Send + Sync>;

/// One pull response: diagnostics paired with the identity of the exact
/// snapshot that produced them (`p:…|d:…|g:…`), per [LSP-PULL-SALSA].
#[derive(Debug, Clone)]
pub struct PullReport {
    /// Opaque identity of the snapshot that produced `items`.
    pub result_id: String,
    /// Diagnostics of that snapshot; empty only on genuine success.
    pub items: Arc<Vec<Diagnostic>>,
}

#[salsa::db]
trait DiagnosticDb: salsa::Database {
    fn analyze(&self, uri: &str) -> Analysis;
}

#[salsa::db]
#[derive(Clone)]
struct Database {
    storage: salsa::Storage<Self>,
    analyzer: Analyzer,
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl DiagnosticDb for Database {
    fn analyze(&self, uri: &str) -> Analysis {
        (self.analyzer)(uri)
    }
}

#[salsa::input]
struct WorkspaceInput {
    generation: u64,
}

#[salsa::input]
struct DocumentInput {
    #[returns(ref)]
    uri: String,
    version: i32,
}

#[salsa::tracked(returns(clone))]
fn analyze(db: &dyn DiagnosticDb, document: DocumentInput, workspace: WorkspaceInput) -> Analysis {
    let _generation = workspace.generation(db);
    let _version = document.version(db);
    db.analyze(document.uri(db))
}

/// Query state serialized behind one mutex. Diagnostics computation is the only
/// accessor; revision changes stay outside so they never queue behind it.
#[derive(Clone)]
struct QueryState {
    database: Database,
    workspace: WorkspaceInput,
    documents: HashMap<String, DocumentInput>,
}

impl QueryState {
    /// Get or create the document handle, then refresh its editor version.
    fn document(&mut self, uri: &str, version: i32) -> DocumentInput {
        let document = *self
            .documents
            .entry(uri.to_owned())
            .or_insert_with(|| DocumentInput::new(&self.database, uri.to_owned(), 0));
        if *document.version(&self.database) != version {
            document.set_version(&mut self.database).to(version);
        }
        document
    }

    /// Apply the pending revision to the workspace input only when changed,
    /// then return the refreshed document handle. [LSP-PULL-CONCURRENCY]
    fn sync(&mut self, revision: u64, uri: &str, version: i32) -> DocumentInput {
        let document = self.document(uri, version);
        if *self.workspace.generation(&self.database) != revision {
            self.workspace
                .set_generation(&mut self.database)
                .to(revision);
        }
        document
    }
}

/// An analysis memo and the input identities for a single LSP connection.
///
/// Cloning shares one Salsa database and one revision counter, so every clone
/// observes the same snapshot identity for the same connection state.
#[derive(Clone)]
pub struct PullStore {
    query: Arc<Mutex<QueryState>>,
    revision: Arc<AtomicU64>,
}

impl PullStore {
    /// Create a session whose compiler is called only for invalidated documents.
    #[must_use]
    pub fn new(analyzer: Analyzer) -> Self {
        let database = Database {
            storage: salsa::Storage::default(),
            analyzer,
        };
        let workspace = WorkspaceInput::new(&database, 1);
        Self {
            query: Arc::new(Mutex::new(QueryState {
                database,
                workspace,
                documents: HashMap::new(),
            })),
            revision: Arc::new(AtomicU64::new(1)),
        }
    }

    /// Invalidate inter-file dependencies with a lock-free revision bump.
    ///
    /// The pending Salsa generation is applied lazily by the next query, so
    /// invalidation never blocks behind a running computation. [LSP-PULL-CONCURRENCY]
    pub fn invalidate(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    /// Current snapshot revision; never touches the query mutex. [LSP-PULL-CONCURRENCY]
    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }

    /// Opaque identity of the snapshot at `version`. Lock-free: inspection
    /// never computes diagnostics nor waits on the query mutex. [LSP-PULL-SALSA]
    #[must_use]
    pub fn result_id(&self, version: i32) -> String {
        identity(self.revision(), version)
    }

    /// Analyze the current snapshot through the single Salsa query. [LSP-PULL-SALSA]
    ///
    /// Locks the query state, captures the revision *after* the lock, applies
    /// that exact revision and document version, and runs the tracked query.
    /// The returned identity and items come from the same captured snapshot;
    /// if the atomic revision moved while computing, the superseded report is
    /// discarded rather than paired with a newer identity. [LSP-PULL-CONCURRENCY]
    ///
    /// # Errors
    /// Returns the compiler failure without converting it into diagnostic
    /// absence; a backend error also invalidates the memo so a later revision
    /// retries the query instead of replaying the failure.
    pub fn diagnostics(&self, uri: &str, version: i32) -> Result<PullReport, AnalysisError> {
        let mut query = self.query.lock().map_err(|_| poisoned())?;
        let revision = self.revision();
        let document = query.sync(revision, uri, version);
        let result = analyze(&query.database, document, query.workspace);
        if self.revision() != revision {
            return Err(superseded());
        }
        if result.is_err() {
            self.invalidate();
        }
        Ok(PullReport {
            result_id: identity(revision, version),
            items: result?,
        })
    }
}

/// Poisoned query lock: an honest failure, never a panic across the boundary.
fn poisoned() -> AnalysisError {
    AnalysisError("diagnostic query lock poisoned".to_owned())
}

/// The snapshot moved on while computing; items and identity would diverge.
fn superseded() -> AnalysisError {
    AnalysisError("diagnostic computation superseded by a newer revision".to_owned())
}

/// Snapshot identity in the wire-stable `p:…|d:…|g:…` form.
fn identity(revision: u64, version: i32) -> String {
    format!("p:{revision}|d:{version}|g:{revision}")
}
