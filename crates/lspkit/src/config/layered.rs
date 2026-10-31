//! Layered TOML config loader.
//!
//! Walks up from a starting directory looking for a config file by name, then
//! deserializes it into a consumer-supplied `serde::Deserialize` type. The
//! search stops at the filesystem root or when a file is found.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;

/// Errors from loading config.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    /// No config file was found while walking up from the start directory.
    #[error("no config file named {0:?} found")]
    NotFound(String),
    /// The file existed but could not be read.
    #[error("could not read {path}: {source}")]
    Io {
        /// Path that failed to open.
        path: PathBuf,
        /// I/O error.
        #[source]
        source: std::io::Error,
    },
    /// The file's TOML could not be parsed.
    #[error("could not parse {path}: {message}")]
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Detail.
        message: String,
    },
}

/// Outcome of a successful load.
#[non_exhaustive]
#[derive(Debug)]
pub struct LoadedConfig<T> {
    /// Path of the file that was loaded.
    pub source: PathBuf,
    /// Deserialized config.
    pub value: T,
}

/// Walk up from `start` looking for `file_name`. Returns the first hit's path,
/// or `None` if the search reaches the filesystem root.
#[must_use]
pub fn find_ancestor(start: &Path, file_name: &str) -> Option<PathBuf> {
    let mut current: Option<&Path> = Some(start);
    while let Some(dir) = current {
        let candidate = dir.join(file_name);
        if candidate.is_file() {
            return Some(candidate);
        }
        current = dir.parent();
    }
    None
}

/// Load and deserialize a config file by walking up from `start`.
///
/// # Errors
/// Returns [`LoadError::NotFound`] if no file matches, [`LoadError::Io`] on
/// read failure, or [`LoadError::Parse`] on invalid TOML.
pub fn load_from_ancestor<T>(start: &Path, file_name: &str) -> Result<LoadedConfig<T>, LoadError>
where
    T: DeserializeOwned,
{
    let path =
        find_ancestor(start, file_name).ok_or_else(|| LoadError::NotFound(file_name.to_owned()))?;
    load_from_path(&path)
}

/// Load and deserialize a config file at a specific path.
///
/// # Errors
/// Returns [`LoadError::Io`] on read failure or [`LoadError::Parse`] on
/// invalid TOML.
pub fn load_from_path<T>(path: &Path) -> Result<LoadedConfig<T>, LoadError>
where
    T: DeserializeOwned,
{
    let text = std::fs::read_to_string(path).map_err(|source| LoadError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let value: T = toml::from_str(&text).map_err(|err| LoadError::Parse {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    Ok(LoadedConfig {
        source: path.to_path_buf(),
        value,
    })
}
