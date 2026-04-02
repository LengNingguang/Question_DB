//! Shared low-level filesystem helpers.

use std::{
    env,
    path::{Path, PathBuf},
};

use anyhow::{bail, Result};

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

pub(crate) fn normalize_bundle_description(field: &str, value: &str) -> Result<String> {
    let normalized = value.trim().to_string();
    validate_bundle_description(field, &normalized)?;
    Ok(normalized)
}

pub(crate) fn normalize_optional_bundle_description(
    field: &str,
    value: Option<String>,
) -> Result<String> {
    let Some(text) = value else {
        bail!("{field} must not be null");
    };
    normalize_bundle_description(field, &text)
}

pub(crate) fn bundle_directory_name(description: &str, id: &str) -> String {
    let suffix = id
        .chars()
        .filter(|ch| *ch != '-')
        .take(6)
        .collect::<String>();
    format!("{description}_{suffix}")
}

fn validate_bundle_description(field: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        bail!("{field} must not be empty");
    }
    if value == "." || value == ".." {
        bail!("{field} must not be '.' or '..'");
    }
    if value.ends_with('.') {
        bail!("{field} must not end with '.'");
    }
    if value.chars().count() > 80 {
        bail!("{field} must be at most 80 characters");
    }

    for ch in value.chars() {
        if ch.is_control() {
            bail!("{field} must not contain control characters");
        }
        if matches!(ch, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
            bail!("{field} contains an invalid filename character: {ch}");
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{bundle_directory_name, normalize_bundle_description};

    #[test]
    fn normalize_bundle_description_accepts_chinese() {
        let normalized =
            normalize_bundle_description("description", "  热学 决赛卷 A  ").expect("valid");
        assert_eq!(normalized, "热学 决赛卷 A");
    }

    #[test]
    fn normalize_bundle_description_rejects_invalid_filename_chars() {
        let err = normalize_bundle_description("description", "bad/name").expect_err("should fail");
        assert!(err.to_string().contains("invalid filename character"));
    }

    #[test]
    fn bundle_directory_name_appends_id_suffix() {
        let directory = bundle_directory_name("热学决赛卷", "550e8400-e29b-41d4-a716-446655440000");
        assert_eq!(directory, "热学决赛卷_550e84");
    }
}
