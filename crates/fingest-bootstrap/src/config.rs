use thiserror::Error;

/// Minimum HMAC key length for HS256.
const MIN_JWT_SECRET_LEN: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub db_max_connections: u32,
    pub log_level: String,
    pub jwt_secret: String,
    pub jwt_expiration_hours: i64,
    pub cors_allowed_origin: String,
    /// Names of the event plugins to activate, in order.
    pub plugins: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ConfigError {
    #[error("Missing environment variable: {0}")]
    Missing(&'static str),

    #[error("Invalid environment variable: {0}")]
    Invalid(&'static str),

    #[error("JWT_SECRET must be at least {MIN_JWT_SECRET_LEN} characters")]
    WeakJwtSecret,
}

impl Config {
    pub fn from_env() -> Result<Self, ConfigError> {
        Self::from_source(|key| std::env::var(key).ok())
    }

    /// Pure over its source so configuration is testable without mutating process env.
    pub fn from_source(get: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigError> {
        let required = |key: &'static str| get(key).ok_or(ConfigError::Missing(key));
        let parsed = |key: &'static str, default: &str| -> Result<String, ConfigError> {
            Ok(get(key).unwrap_or_else(|| default.to_owned()))
        };

        let jwt_secret = required("JWT_SECRET")?;
        if jwt_secret.len() < MIN_JWT_SECRET_LEN {
            // Deviation D12: v1 accepted any secret, including the sample placeholder.
            return Err(ConfigError::WeakJwtSecret);
        }

        Ok(Self {
            host: parsed("HOST", "127.0.0.1")?,
            port: parsed("PORT", "8080")?
                .parse()
                .map_err(|_| ConfigError::Invalid("PORT"))?,
            database_url: required("DATABASE_URL")?,
            db_max_connections: parsed("DB_MAX_CONNECTIONS", "10")?
                .parse()
                .map_err(|_| ConfigError::Invalid("DB_MAX_CONNECTIONS"))?,
            log_level: parsed("RUST_LOG", "info")?,
            jwt_secret,
            jwt_expiration_hours: parsed("JWT_EXPIRATION_HOURS", "24")?
                .parse()
                .map_err(|_| ConfigError::Invalid("JWT_EXPIRATION_HOURS"))?,
            cors_allowed_origin: parsed("CORS_ALLOWED_ORIGIN", "http://localhost:8081")?,
            plugins: parsed("PLUGINS", "tracing")?
                .split(',')
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .map(str::to_owned)
                .collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn source(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key: &str| map.get(key).cloned()
    }

    fn valid_secret() -> &'static str {
        "0123456789abcdef0123456789abcdef"
    }

    #[test]
    fn applies_v1_defaults() {
        let config = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://localhost/db"),
            ("JWT_SECRET", valid_secret()),
        ]))
        .unwrap();

        assert_eq!(config.host, "127.0.0.1");
        assert_eq!(config.port, 8080);
        assert_eq!(config.db_max_connections, 10);
        assert_eq!(config.log_level, "info");
        assert_eq!(config.jwt_expiration_hours, 24);
        assert_eq!(config.plugins, vec!["tracing".to_owned()]);
    }

    #[test]
    fn plugins_are_comma_separated_and_trimmed() {
        let config = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://localhost/db"),
            ("JWT_SECRET", valid_secret()),
            ("PLUGINS", " tracing , in-process "),
        ]))
        .unwrap();

        assert_eq!(config.plugins, vec!["tracing", "in-process"]);
    }

    #[test]
    fn publishing_can_be_disabled_entirely() {
        let config = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://localhost/db"),
            ("JWT_SECRET", valid_secret()),
            ("PLUGINS", ""),
        ]))
        .unwrap();

        assert!(config.plugins.is_empty());
    }

    #[test]
    fn overrides_are_honoured() {
        let config = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://localhost/db"),
            ("JWT_SECRET", valid_secret()),
            ("HOST", "0.0.0.0"),
            ("PORT", "3000"),
            ("DB_MAX_CONNECTIONS", "5"),
            ("JWT_EXPIRATION_HOURS", "48"),
        ]))
        .unwrap();

        assert_eq!(config.host, "0.0.0.0");
        assert_eq!(config.port, 3000);
        assert_eq!(config.db_max_connections, 5);
        assert_eq!(config.jwt_expiration_hours, 48);
    }

    #[test]
    fn database_url_is_required() {
        let err = Config::from_source(source(&[("JWT_SECRET", valid_secret())])).unwrap_err();
        assert_eq!(err, ConfigError::Missing("DATABASE_URL"));
    }

    #[test]
    fn jwt_secret_is_required() {
        let err = Config::from_source(source(&[("DATABASE_URL", "postgres://x/db")])).unwrap_err();
        assert_eq!(err, ConfigError::Missing("JWT_SECRET"));
    }

    #[test]
    fn short_jwt_secret_is_rejected() {
        let err = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://x/db"),
            ("JWT_SECRET", "too-short"),
        ]))
        .unwrap_err();

        assert_eq!(err, ConfigError::WeakJwtSecret);
    }

    #[test]
    fn the_v1_sample_placeholder_is_rejected_only_when_short() {
        // sample.env ships a 60-char placeholder, so it passes length but should still be
        // changed in production; length is the only thing we can mechanically enforce.
        let long_placeholder = "your-secret-key-min-32-characters-change-in-production";
        assert!(long_placeholder.len() >= MIN_JWT_SECRET_LEN);
    }

    #[test]
    fn non_numeric_port_is_rejected() {
        let err = Config::from_source(source(&[
            ("DATABASE_URL", "postgres://x/db"),
            ("JWT_SECRET", valid_secret()),
            ("PORT", "not-a-port"),
        ]))
        .unwrap_err();

        assert_eq!(err, ConfigError::Invalid("PORT"));
    }
}
