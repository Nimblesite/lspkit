//! `EngineApi` — the contract every engine implementation satisfies.
//!
//! One engine instance is consumed by both the LSP server scaffolding and the
//! MCP adapter. See `spec.md` for the full behavioral contract (monotonicity,
//! snapshot honesty, cancellation propagation, shutdown semantics).

use std::error::Error as StdError;
use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use tokio_util::sync::CancellationToken;

/// Monotonically increasing version counter for an engine's state.
///
/// Two successful rescans produce two distinct, ordered generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Generation(u64);

impl Generation {
    /// First generation. New engines start here.
    pub const ZERO: Self = Self(0);

    /// Wrap a raw `u64`.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// The next generation. Saturates at `u64::MAX`.
    #[must_use]
    pub const fn next(self) -> Self {
        Self(self.0.saturating_add(1))
    }

    /// Underlying integer value.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "gen#{}", self.0)
    }
}

/// A report payload tagged with the generation that produced it.
///
/// Comparing `generation` across snapshots is the contract for detecting staleness.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct Snapshot<R> {
    /// Generation at which `data` was computed.
    pub generation: Generation,
    /// The engine-defined report payload.
    pub data: R,
}

impl<R> Snapshot<R> {
    /// Wrap `data` in a snapshot tagged with `generation`.
    pub const fn new(generation: Generation, data: R) -> Self {
        Self { generation, data }
    }
}

/// Reason a `GenerationEvent` was emitted.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum Cause {
    /// Initial state observed at construction.
    Initial,
    /// A rescan completed.
    Rescan,
    /// External invalidation (e.g. config reload).
    External,
}

/// Notification emitted when an engine advances to a new generation.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct GenerationEvent {
    /// The generation now in effect.
    pub generation: Generation,
    /// Why this event was emitted.
    pub cause: Cause,
}

impl GenerationEvent {
    /// Construct an event.
    #[must_use]
    pub const fn new(generation: Generation, cause: Cause) -> Self {
        Self { generation, cause }
    }
}

/// Scope of a rescan request.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum RescanScope {
    /// Rescan every watched root.
    All,
    /// Rescan only the listed paths.
    Paths(Vec<PathBuf>),
}

/// Handle returned from a successful `rescan` call.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub struct RescanTicket {
    generation: Generation,
}

impl RescanTicket {
    /// Construct a ticket tagged with the generation the rescan will produce.
    #[must_use]
    pub const fn new(generation: Generation) -> Self {
        Self { generation }
    }

    /// The generation the rescan will (or did) produce.
    #[must_use]
    pub const fn generation(&self) -> Generation {
        self.generation
    }
}

/// Progress sink for long-running engine operations.
///
/// Wraps an LSP `window/workDoneProgress` reporter or a no-op when the client
/// did not advertise the capability. Use `Progress::noop()` in non-LSP contexts.
#[derive(Clone)]
pub struct Progress {
    inner: ProgressInner,
}

#[derive(Clone)]
enum ProgressInner {
    Noop,
}

impl Progress {
    /// A `Progress` that drops every report.
    #[must_use]
    pub fn noop() -> Self {
        Self {
            inner: ProgressInner::Noop,
        }
    }

    /// Report a progress message and an optional percentage in `[0, 100]`.
    pub fn report(&self, _message: &str, _percentage: Option<u32>) {
        match self.inner {
            ProgressInner::Noop => {}
        }
    }
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Progress").finish_non_exhaustive()
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::noop()
    }
}

/// Boxed stream of `GenerationEvent` values returned by `EngineApi::subscribe`.
pub type GenerationEventStream = Pin<Box<dyn Stream<Item = GenerationEvent> + Send + 'static>>;

/// The contract every engine implementation satisfies.
///
/// One implementation is consumed by both the LSP server scaffolding in
/// `lspkit-server` and the MCP adapter in `lspkit-mcp`. See `spec.md` for the
/// full behavioral contract.
#[async_trait]
pub trait EngineApi: Send + Sync + 'static {
    /// Engine-defined report payload type.
    type Report: Send + Sync + 'static;
    /// Engine-defined query type. A report is computed for a given query.
    type Query: Send + Sync;
    /// Engine-defined error type.
    type Error: StdError + Send + Sync + 'static;

    /// Current monotonic generation. Increases on every successful rescan.
    fn generation(&self) -> Generation;

    /// Compute a report for `query`. MUST observe `cancel` and return a
    /// cancellation error within a bounded delay.
    async fn report(
        &self,
        query: Self::Query,
        cancel: CancellationToken,
    ) -> Result<Snapshot<Self::Report>, Self::Error>;

    /// Request a rescan of `scope`. Implementations MAY coalesce concurrent
    /// requests; `progress` MAY emit zero events.
    async fn rescan(
        &self,
        scope: RescanScope,
        progress: Progress,
    ) -> Result<RescanTicket, Self::Error>;

    /// Subscribe to generation-change notifications.
    fn subscribe(&self) -> GenerationEventStream;

    /// Drain in-flight work, complete subscriber streams, and refuse further
    /// calls. After `shutdown` returns, every other method MUST return
    /// `Self::Error`.
    async fn shutdown(&self) -> Result<(), Self::Error>;
}
