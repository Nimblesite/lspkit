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
use tokio_util::sync::CancellationToken;

/// A backend failure must remain an error, never an empty clean report.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("diagnostic analysis unavailable: {0}")]
pub struct AnalysisError(pub String);

/// Diagnostics for one current compiler snapshot, or an honest failure.
pub type Analysis = Result<Arc<Vec<Diagnostic>>, AnalysisError>;

/// A stale snapshot can be retried without hiding an actual backend failure.
/// Implements [LSP-PULL-CONCURRENCY].
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum PullError {
    /// The compiler or query state is unavailable.
    #[error(transparent)]
    Analysis(#[from] AnalysisError),
    /// The completed query belongs to an input revision that has been replaced.
    #[error("diagnostic computation superseded by a newer revision")]
    Superseded,
    /// The caller abandoned this query; it must never become successful absence.
    #[error("diagnostic request cancelled")]
    Cancelled,
}

/// The compiler callback, invoked only on a Salsa miss.
pub type Analyzer = Arc<dyn Fn(&str) -> Analysis + Send + Sync>;

/// Compiler callback carrying the actual request lifetime. [LSP-PULL-CANCELLATION]
pub type CancellableAnalyzer = Arc<dyn Fn(&str, &CancellationToken) -> Analysis + Send + Sync>;

/// One pull response: diagnostics paired with the identity of the exact
/// snapshot that produced them (`p:…|d:…|g:…`), per [LSP-PULL-SALSA].
#[derive(Debug, Clone)]
pub struct PullReport {
    /// Opaque identity of the snapshot that produced `items`.
    pub result_id: String,
    /// Diagnostics of that snapshot; empty only on genuine success.
    pub items: Arc<Vec<Diagnostic>>,
}

impl PullReport {
    /// Pair successful analysis with the exact input identity that produced it.
    fn computed(revision: u64, version: i32, result: Analysis) -> Result<Self, PullError> {
        Ok(Self {
            result_id: identity(revision, version),
            items: result?,
        })
    }
}

/// Cancellation never becomes successful diagnostic absence.
fn uncancelled(cancel: &CancellationToken) -> Result<(), PullError> {
    if cancel.is_cancelled() {
        Err(PullError::Cancelled)
    } else {
        Ok(())
    }
}

#[salsa::db]
trait DiagnosticDb: salsa::Database {
    fn analyze(&self, uri: &str) -> Analysis;
}

#[salsa::db]
#[derive(Clone)]
struct Database {
    storage: salsa::Storage<Self>,
    analyzer: CancellableAnalyzer,
    cancellation: CancellationToken,
}

#[salsa::db]
impl salsa::Database for Database {}

#[salsa::db]
impl DiagnosticDb for Database {
    fn analyze(&self, uri: &str) -> Analysis {
        (self.analyzer)(uri, &self.cancellation)
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
    retry_revision: u64,
}

#[salsa::tracked(returns(clone))]
fn analyze(db: &dyn DiagnosticDb, document: DocumentInput, workspace: WorkspaceInput) -> Analysis {
    let _generation = workspace.generation(db);
    let _version = document.version(db);
    let _retry = document.retry_revision(db);
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
            .or_insert_with(|| DocumentInput::new(&self.database, uri.to_owned(), 0, 0));
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

    /// Failed attempts invalidate only their Salsa query, preserving unrelated truth.
    fn retry(&mut self, document: DocumentInput) {
        // This private counter need only change; wrapping cannot reuse the current input value.
        let next = document.retry_revision(&self.database).wrapping_add(1);
        document.set_retry_revision(&mut self.database).to(next);
    }

    /// Cancellation belongs to execution context, never a second diagnostic cache.
    fn compute(
        &mut self,
        uri: &str,
        version: i32,
        revision: u64,
        cancel: &CancellationToken,
    ) -> Result<PullReport, PullError> {
        uncancelled(cancel)?;
        let document = self.sync(revision, uri, version);
        self.database.cancellation = cancel.clone();
        let result = analyze(&self.database, document, self.workspace);
        if result.is_err() {
            self.retry(document);
        }
        uncancelled(cancel)?;
        PullReport::computed(revision, version, result)
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
        Self::new_cancellable(Arc::new(move |uri, _cancel| analyzer(uri)))
    }

    /// Create a session that propagates cancellation through actual compiler work.
    #[must_use]
    pub fn new_cancellable(analyzer: CancellableAnalyzer) -> Self {
        let database = Database {
            storage: salsa::Storage::default(),
            analyzer,
            cancellation: CancellationToken::new(),
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
    pub fn diagnostics(&self, uri: &str, version: i32) -> Result<PullReport, PullError> {
        self.diagnostics_cancellable(uri, version, &CancellationToken::new())
    }

    /// Query with the caller's lifetime; a cancelled attempt is never memoized as clean.
    ///
    /// # Errors
    /// Returns cancellation, supersession, or the real backend failure.
    pub fn diagnostics_cancellable(
        &self,
        uri: &str,
        version: i32,
        cancel: &CancellationToken,
    ) -> Result<PullReport, PullError> {
        uncancelled(cancel)?;
        let mut query = self.query.lock().map_err(|_| poisoned())?;
        let revision = self.revision();
        let report = query.compute(uri, version, revision, cancel)?;
        if self.revision() != revision {
            return Err(PullError::Superseded);
        }
        Ok(report)
    }
}

/// Poisoned query lock: an honest failure, never a panic across the boundary.
fn poisoned() -> AnalysisError {
    AnalysisError("diagnostic query lock poisoned".to_owned())
}

/// Snapshot identity in the wire-stable `p:…|d:…|g:…` form.
fn identity(revision: u64, version: i32) -> String {
    format!("p:{revision}|d:{version}|g:{revision}")
}
