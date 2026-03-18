use std::{env, net::SocketAddr};

use anyhow::{Context, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppConfig {
    pub database_url: String,
    pub bind_addr: SocketAddr,
}

impl AppConfig {
    pub fn from_env() -> Result<Self> {
        let database_url =
            env::var("QB_DATABASE_URL").context("QB_DATABASE_URL is required for Rust API")?;

        let bind_addr = env::var("QB_BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".to_string())
            .parse()
            .context("QB_BIND_ADDR must be a valid socket address, e.g. 127.0.0.1:8080")?;

        Ok(Self {
            database_url,
            bind_addr,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::AppConfig;
    use std::env;

    /// Guard that restores a single environment variable to its prior state on drop.
    struct EnvVarGuard {
        key: &'static str,
        prev: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let prev = env::var(key).ok();
            env::set_var(key, value);
            EnvVarGuard { key, prev }
        }

        fn remove(key: &'static str) -> Self {
            let prev = env::var(key).ok();
            env::remove_var(key);
            EnvVarGuard { key, prev }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.prev {
                Some(val) => env::set_var(self.key, val),
                None => env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn config_reads_env_and_uses_default_bind_addr() {
        let _db_guard = EnvVarGuard::set(
            "QB_DATABASE_URL",
            "postgres://postgres:postgres@localhost/qb",
        );
        let _bind_guard = EnvVarGuard::remove("QB_BIND_ADDR");

        let cfg = AppConfig::from_env().expect("config should load");
        assert_eq!(cfg.bind_addr.to_string(), "127.0.0.1:8080");
        assert_eq!(
            cfg.database_url,
            "postgres://postgres:postgres@localhost/qb"
        );
    }
}
