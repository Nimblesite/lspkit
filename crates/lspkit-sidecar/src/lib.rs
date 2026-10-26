//! `lspkit-sidecar` — framed-IPC primitives and lifecycle management for
//! out-of-process backends.
//!
//! **Pure transport.** This crate has no opinion on the wire format
//! (`MessagePack`, JSON, CBOR — caller's choice), no knowledge of any specific
//! backend technology, and no protocol payload types. Three layers:
//!
//! - [`transport`] — 4-byte little-endian length-prefixed frames.
//! - [`correlator`] — request-ID → response-future routing.
//! - [`lifecycle`] — spawn / health-ping / exponential-backoff restart.

pub mod cancellation;
pub mod correlator;
pub mod inbound;
pub mod lifecycle;
pub mod transport;

pub use crate::correlator::{CorrelationError, Correlator, RequestId};
pub use crate::lifecycle::{LifecycleConfig, LifecycleError, Sidecar};
pub use crate::transport::{read_frame, write_frame, TransportError, DEFAULT_MAX_FRAME};
