//! Shared low-level filesystem helpers.

use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) fn expand_path(input: &str) -> PathBuf {
    if input == "~" {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    if let Some(stripped) = input.strip_prefix("~/") {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(input)
}

pub(crate) fn canonical_or_original(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .to_string()
}
