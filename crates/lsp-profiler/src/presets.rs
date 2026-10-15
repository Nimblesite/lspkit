//! Named sampling presets. Implements [PROF-CPU-PRESETS].
//!
//! A preset bundles a sample rate and an optional duration limit so users pick
//! a mode instead of tuning parameters. Explicit request parameters override
//! the preset's values.

use std::time::Duration;

use crate::runtime::SamplerConfig;

/// Default sampling frequency in hertz.
pub const DEFAULT_SAMPLE_RATE: u64 = 100;
/// Highest accepted frequency; prevents zero-length timer hot loops.
pub const MAX_SAMPLE_RATE: u64 = 10_000;

/// Preset names accepted on the wire.
pub mod names {
    /// 10 seconds at 100 Hz, for quick hotspot checks.
    pub const QUICK: &str = "quick";
    /// 60 seconds at 200 Hz, higher-fidelity data.
    pub const DETAILED: &str = "detailed";
    /// No time limit at 50 Hz, for long-running servers and batch jobs.
    pub const LONG_RUNNING: &str = "longRunning";
    /// Every preset name.
    pub const ALL: &[&str] = &[QUICK, DETAILED, LONG_RUNNING];
}

/// Pre-configured profiling modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfilingPreset {
    /// 10 seconds at 100 Hz.
    Quick,
    /// 60 seconds at 200 Hz.
    Detailed,
    /// Unlimited at 50 Hz.
    LongRunning,
}

impl ProfilingPreset {
    /// Parse a preset name; `None` for anything else.
    #[must_use]
    pub fn parse_name(name: &str) -> Option<Self> {
        match name {
            names::QUICK => Some(Self::Quick),
            names::DETAILED => Some(Self::Detailed),
            names::LONG_RUNNING | "long_running" => Some(Self::LongRunning),
            _ => None,
        }
    }

    /// The preset's sample rate and duration limit.
    #[must_use]
    pub fn rate_and_duration(self) -> (u64, Option<Duration>) {
        match self {
            Self::Quick => (100, Some(Duration::from_secs(10))),
            Self::Detailed => (200, Some(Duration::from_secs(60))),
            Self::LongRunning => (50, None),
        }
    }
}

/// Requested sampling parameters, each optional.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SamplingRequest {
    /// Explicit rate.
    pub sample_rate: Option<u64>,
    /// Explicit native-frames flag.
    pub include_native: Option<bool>,
    /// Explicit duration limit.
    pub duration: Option<Duration>,
    /// Preset name.
    pub preset: Option<String>,
}

/// Resolve a request into a validated [`SamplerConfig`]: explicit values win
/// over the preset, the preset over the defaults.
///
/// # Errors
///
/// Returns a message when the rate is outside `1..=MAX_SAMPLE_RATE` or the
/// preset name is unknown.
pub fn resolve(pid: u32, request: &SamplingRequest) -> Result<SamplerConfig, String> {
    let preset = match request.preset.as_deref() {
        None | Some("default") => None,
        Some(name) => Some(ProfilingPreset::parse_name(name).ok_or_else(|| {
            format!(
                "unknown preset '{name}'; expected one of {}",
                names::ALL.join(", ")
            )
        })?),
    };
    let (preset_rate, preset_duration) = preset.map_or(
        (DEFAULT_SAMPLE_RATE, None),
        ProfilingPreset::rate_and_duration,
    );
    let sample_rate = request.sample_rate.unwrap_or(preset_rate);
    if !(1..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(format!(
            "sample rate must be between 1 and {MAX_SAMPLE_RATE} Hz"
        ));
    }
    Ok(SamplerConfig {
        pid,
        sample_rate,
        include_native: request.include_native.unwrap_or(false),
        include_idle: false,
        duration: request.duration.or(preset_duration),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(name: &str) -> SamplingRequest {
        SamplingRequest {
            preset: Some(name.to_owned()),
            ..SamplingRequest::default()
        }
    }

    #[test]
    fn presets_set_rate_and_duration() -> Result<(), String> {
        let quick = resolve(1234, &preset("quick"))?;
        assert_eq!(
            (quick.pid, quick.sample_rate, quick.duration),
            (1234, 100, Some(Duration::from_secs(10)))
        );
        let detailed = resolve(1, &preset("detailed"))?;
        assert_eq!(
            (detailed.sample_rate, detailed.duration),
            (200, Some(Duration::from_secs(60)))
        );
        let long = resolve(1, &preset("long_running"))?;
        assert_eq!((long.sample_rate, long.duration), (50, None));
        Ok(())
    }

    #[test]
    fn explicit_values_override_the_preset() -> Result<(), String> {
        let request = SamplingRequest {
            sample_rate: Some(333),
            include_native: Some(true),
            duration: Some(Duration::from_secs(3)),
            preset: Some("detailed".to_owned()),
        };
        let config = resolve(9, &request)?;
        assert_eq!(
            (config.sample_rate, config.include_native, config.duration),
            (333, true, Some(Duration::from_secs(3)))
        );
        Ok(())
    }

    #[test]
    fn defaults_apply_without_preset() -> Result<(), String> {
        let config = resolve(9, &SamplingRequest::default())?;
        assert_eq!(
            (config.sample_rate, config.include_native, config.duration),
            (DEFAULT_SAMPLE_RATE, false, None)
        );
        assert_eq!(
            resolve(9, &preset("default"))?.sample_rate,
            DEFAULT_SAMPLE_RATE
        );
        Ok(())
    }

    #[test]
    fn rejects_bad_rates_and_unknown_presets() {
        let zero = SamplingRequest {
            sample_rate: Some(0),
            ..SamplingRequest::default()
        };
        assert!(resolve(1, &zero).is_err_and(|e| e.contains("between 1 and 10000")));
        let fast = SamplingRequest {
            sample_rate: Some(MAX_SAMPLE_RATE + 1),
            ..SamplingRequest::default()
        };
        assert!(resolve(1, &fast).is_err());
        assert!(resolve(1, &preset("turbo")).is_err_and(|e| e.contains("unknown preset 'turbo'")));
    }
}
