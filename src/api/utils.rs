//! Low-level filesystem, serialization, and content normalization helpers.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

pub(crate) static LATEX_CMD_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\\[A-Za-z@]+").expect("valid regex"));
pub(crate) static SPECIAL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[{}\[\]$^_&%#~]").expect("valid regex"));
pub(crate) static WHITESPACE_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s+").expect("valid regex"));

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

pub(crate) fn join_bundle_path(bundle_path: &Path, rel_or_abs: &str) -> PathBuf {
    let candidate = PathBuf::from(rel_or_abs);
    if candidate.is_absolute() {
        candidate
    } else {
        bundle_path.join(candidate)
    }
}

pub(crate) fn missing_keys(map: &Map<String, Value>, required_keys: &[&str]) -> Vec<String> {
    required_keys
        .iter()
        .filter(|key| !map.contains_key(**key))
        .map(|key| key.to_string())
        .collect()
}

pub(crate) fn strip_utf8_bom(bytes: &[u8]) -> &[u8] {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        bytes
    }
}

pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let bytes = fs::read(path)
        .with_context(|| format!("read json file failed: {}", path.to_string_lossy()))?;
    let stripped = strip_utf8_bom(&bytes);
    let parsed = serde_json::from_slice(stripped)
        .with_context(|| format!("parse json file failed: {}", path.to_string_lossy()))?;
    Ok(parsed)
}

pub(crate) fn normalize_search_text(parts: &[Option<&str>], limit: usize) -> String {
    let merged = parts
        .iter()
        .flatten()
        .copied()
        .collect::<Vec<_>>()
        .join(" ");
    let without_cmd = LATEX_CMD_RE.replace_all(&merged, " ");
    let without_special = SPECIAL_RE.replace_all(&without_cmd, " ");
    let compact = WHITESPACE_RE
        .replace_all(&without_special, " ")
        .trim()
        .to_string();
    if compact.chars().count() > limit {
        compact.chars().take(limit).collect()
    } else {
        compact
    }
}

pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize()).to_uppercase()
}
