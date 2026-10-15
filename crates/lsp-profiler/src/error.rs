//! Profiler errors and their JSON-RPC codes. Implements [PROF-ERRORS].

use crate::memory::MemoryError;
use crate::sampler::SamplerError;
use crate::wire::error_codes;

/// Any failure of a profiler request.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ProfilerError {
    /// A sampler could not attach.
    #[error(transparent)]
    Sampler(#[from] SamplerError),
    /// A memory step failed.
    #[error(transparent)]
    Memory(#[from] MemoryError),
    /// The PID already has a CPU session.
    #[error("Already profiling PID {pid} (session {session_id})")]
    AlreadyProfiling {
        /// The PID.
        pid: u32,
        /// Its existing session.
        session_id: String,
    },
    /// No such CPU session.
    #[error("No active profiling session: {0}")]
    SessionNotFound(String),
    /// No registered runtime recognises the process, or the named runtime is unknown.
    #[error("{0}")]
    NoRuntime(String),
    /// The request parameters are invalid.
    #[error("invalid parameters: {0}")]
    InvalidParams(String),
    /// Anything else.
    #[error("{0}")]
    Other(String),
}

impl ProfilerError {
    /// The JSON-RPC error code for this error.
    #[must_use]
    pub fn code(&self) -> i64 {
        match self {
            Self::Sampler(SamplerError::ProcessNotFound(_)) => error_codes::PROCESS_NOT_FOUND,
            Self::Sampler(SamplerError::UnsupportedRuntime { .. }) | Self::NoRuntime(_) => {
                error_codes::UNSUPPORTED_RUNTIME
            }
            Self::Sampler(SamplerError::PermissionDenied(_)) => error_codes::PERMISSION_DENIED,
            Self::AlreadyProfiling { .. } => error_codes::ALREADY_PROFILING,
            Self::Sampler(SamplerError::AttachFailed(_)) => error_codes::ATTACH_FAILED,
            Self::Sampler(SamplerError::ToolMissing(_))
            | Self::Memory(MemoryError::ToolMissing(_)) => error_codes::TOOL_MISSING,
            Self::InvalidParams(_) => error_codes::INVALID_PARAMS,
            Self::Memory(_) | Self::SessionNotFound(_) | Self::Other(_) => error_codes::OTHER,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_every_failure_to_its_code() {
        let cases: Vec<(ProfilerError, i64)> = vec![
            (SamplerError::ProcessNotFound(1).into(), -32001),
            (
                SamplerError::UnsupportedRuntime {
                    pid: 1,
                    runtime: "X".to_owned(),
                }
                .into(),
                -32002,
            ),
            (ProfilerError::NoRuntime("none".to_owned()), -32002),
            (
                SamplerError::PermissionDenied("p".to_owned()).into(),
                -32003,
            ),
            (
                ProfilerError::AlreadyProfiling {
                    pid: 1,
                    session_id: "s".to_owned(),
                },
                -32004,
            ),
            (SamplerError::AttachFailed("a".to_owned()).into(), -32005),
            (
                SamplerError::ToolMissing("install".to_owned()).into(),
                -32006,
            ),
            (
                MemoryError::ToolMissing("install".to_owned()).into(),
                -32006,
            ),
            (ProfilerError::InvalidParams("bad".to_owned()), -32602),
            (ProfilerError::SessionNotFound("s".to_owned()), -32000),
            (MemoryError::UnknownSession("m".to_owned()).into(), -32000),
        ];
        for (error, code) in cases {
            assert_eq!(error.code(), code, "{error}");
        }
    }

    #[test]
    fn messages_name_the_cause() {
        assert_eq!(
            ProfilerError::AlreadyProfiling {
                pid: 7,
                session_id: "prof-1".to_owned()
            }
            .to_string(),
            "Already profiling PID 7 (session prof-1)"
        );
        assert_eq!(
            ProfilerError::from(SamplerError::UnsupportedRuntime {
                pid: 3,
                runtime: "Python".to_owned()
            })
            .to_string(),
            "PID 3 is not a Python process"
        );
    }
}
