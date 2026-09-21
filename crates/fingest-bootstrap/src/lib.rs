//! Composition root.
//!
//! The only crate that names both a port and its concrete adapter. Everything above this
//! line sees traits; everything below sees Postgres.

pub mod config;

use std::sync::Arc;
use std::time::Duration;

use actix_cors::Cors;
use actix_web::{App, HttpServer, middleware::Logger, web};
use fingest_auth_jwt::{BcryptHasher, JwtTokens};
use fingest_catalog_core::{CategoryRepository, CategoryService};
use fingest_catalog_pg::PgCategoryRepository;
use fingest_events::{InProcessPublisher, OutboxRelay, PgOutboxReader, TracingPublisher};
use fingest_http::{CapabilityReport, TokenVerifierRef};
use fingest_identity_core::{
    AccountRepository, AuthService, PasswordHasher, TokenIssuer, TokenVerifier, UserService,
};
use fingest_identity_pg::PgAccountRepository;
use fingest_kernel::{Clock, EventPublisher, PortError, SystemClock};
use fingest_planning_core::{BudgetRepository, BudgetService};
use fingest_planning_pg::PgBudgetRepository;
use fingest_plugins::{Plugin, PluginError, PluginHost, Registry};
use fingest_wallets_core::{UnitOfWork, WalletReader, WalletService};
use fingest_wallets_pg::{PgUnitOfWork, PgWalletReader};
use sqlx::postgres::{PgPool, PgPoolOptions};
use thiserror::Error;

pub use config::{Config, ConfigError};

/// How often the outbox relay looks for new rows.
const OUTBOX_POLL_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Debug, Error)]
pub enum BootstrapError {
    #[error(transparent)]
    Config(#[from] ConfigError),

    #[error("Database connection failed: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Migration failed: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("Adapter setup failed: {0}")]
    Port(#[from] PortError),

    #[error("Plugin setup failed: {0}")]
    Plugin(#[from] PluginError),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Wired dependencies, cloned into each actix worker.
#[derive(Clone)]
pub struct Dependencies {
    category_service: web::Data<CategoryService>,
    auth_service: web::Data<AuthService>,
    user_service: web::Data<UserService>,
    wallet_service: web::Data<WalletService>,
    budget_service: web::Data<BudgetService>,
    token_verifier: web::Data<TokenVerifierRef>,
    capabilities: web::Data<CapabilityReport>,
}

impl Dependencies {
    pub fn build(
        pool: PgPool,
        config: &Config,
        capabilities: CapabilityReport,
    ) -> Result<Self, BootstrapError> {
        let clock: Arc<dyn Clock> = Arc::new(SystemClock);
        let clock_for_wallets = Arc::clone(&clock);

        let tokens = Arc::new(JwtTokens::new(
            &config.jwt_secret,
            config.jwt_expiration_hours,
            clock,
        ));
        let hasher: Arc<dyn PasswordHasher> =
            Arc::new(BcryptHasher::new(BcryptHasher::DEFAULT_COST)?);
        let accounts: Arc<dyn AccountRepository> = Arc::new(PgAccountRepository::new(pool.clone()));
        let categories: Arc<dyn CategoryRepository> =
            Arc::new(PgCategoryRepository::new(pool.clone()));
        let wallet_reader: Arc<dyn WalletReader> = Arc::new(PgWalletReader::new(pool.clone()));
        let wallet_uow: Arc<dyn UnitOfWork> = Arc::new(PgUnitOfWork::new(pool.clone()));
        let budgets: Arc<dyn BudgetRepository> = Arc::new(PgBudgetRepository::new(pool));

        let issuer: Arc<dyn TokenIssuer> = tokens.clone();
        let verifier: Arc<dyn TokenVerifier> = tokens;

        Ok(Self {
            category_service: web::Data::new(CategoryService::new(categories)),
            auth_service: web::Data::new(AuthService::new(
                Arc::clone(&accounts),
                hasher,
                issuer,
                Arc::clone(&verifier),
            )),
            user_service: web::Data::new(UserService::new(accounts)),
            wallet_service: web::Data::new(WalletService::new(
                wallet_reader,
                wallet_uow,
                Arc::clone(&clock_for_wallets),
            )),
            budget_service: web::Data::new(BudgetService::new(budgets, clock_for_wallets)),
            token_verifier: web::Data::new(TokenVerifierRef(verifier)),
            capabilities: web::Data::new(capabilities),
        })
    }

    pub fn register(&self, cfg: &mut web::ServiceConfig) {
        cfg.app_data(self.category_service.clone())
            .app_data(self.auth_service.clone())
            .app_data(self.user_service.clone())
            .app_data(self.wallet_service.clone())
            .app_data(self.budget_service.clone())
            .app_data(self.token_verifier.clone())
            .app_data(self.capabilities.clone())
            .app_data(fingest_http::json_config_plain());
        fingest_http::configure_routes(cfg, Arc::clone(&self.token_verifier.0));
    }
}

pub async fn connect(config: &Config) -> Result<PgPool, BootstrapError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .connect(&config.database_url)
        .await?;

    sqlx::migrate!("../../migrations").run(&pool).await?;

    Ok(pool)
}

pub fn init_tracing(log_level: &str) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(log_level.to_owned())
        .try_init();
}

/// Publishes outbox rows in the background. Detached on purpose: a publishing outage must
/// not stop the API from serving requests, and unpublished rows simply wait.
fn spawn_outbox_relay(pool: PgPool, publisher: Arc<dyn EventPublisher>) {
    let relay = OutboxRelay::new(Arc::new(PgOutboxReader::new(pool)), publisher);

    tokio::spawn(relay.run(OUTBOX_POLL_INTERVAL));
}

/// Writes every event to the log.
struct TracingPlugin;

impl Plugin for TracingPlugin {
    fn name(&self) -> &'static str {
        "tracing"
    }
    fn register(&self, registry: &mut Registry) {
        registry.add_publisher(Arc::new(TracingPublisher));
    }
}

/// Fans events out to in-process subscribers.
struct InProcessPlugin;

impl Plugin for InProcessPlugin {
    fn name(&self) -> &'static str {
        "in-process"
    }
    fn register(&self, registry: &mut Registry) {
        registry.add_publisher(Arc::new(InProcessPublisher::default()));
    }
}

/// Every plugin compiled into this binary. Configuration selects which run.
fn plugin_host() -> Result<PluginHost, BootstrapError> {
    let mut host = PluginHost::new();
    host.register(Box::new(TracingPlugin))?;
    host.register(Box::new(InProcessPlugin))?;
    Ok(host)
}

/// Activates the configured plugins, yielding the publisher and what clients may ask about.
fn wire_plugins(
    config: &Config,
) -> Result<(Arc<dyn EventPublisher>, CapabilityReport), BootstrapError> {
    let host = plugin_host()?;
    let available = host.available().into_iter().map(str::to_owned).collect();
    let registry = host.build(&config.plugins)?;

    let report = CapabilityReport {
        enabled: config.plugins.clone(),
        available,
        capabilities: registry.capabilities().to_vec(),
    };

    Ok((registry.into_publisher(), report))
}

pub async fn run(config: Config) -> Result<(), BootstrapError> {
    let pool = connect(&config).await?;

    let (publisher, capabilities) = wire_plugins(&config)?;
    spawn_outbox_relay(pool.clone(), publisher);

    let dependencies = Dependencies::build(pool, &config, capabilities)?;
    let allowed_origin = config.cors_allowed_origin.clone();

    tracing::info!(host = %config.host, port = config.port, "starting server");

    HttpServer::new(move || {
        let cors = Cors::default()
            .allowed_origin(&allowed_origin)
            .allowed_methods(vec!["GET", "POST", "PUT", "DELETE"])
            .allowed_headers(vec!["Authorization", "Content-Type"])
            .supports_credentials()
            .max_age(3600);

        App::new()
            .wrap(Logger::default())
            .wrap(cors)
            .configure(|cfg| dependencies.register(cfg))
    })
    .bind((config.host.as_str(), config.port))?
    .run()
    .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(plugins: &str) -> Config {
        Config::from_source(|key| match key {
            "DATABASE_URL" => Some("postgres://localhost/db".to_owned()),
            "JWT_SECRET" => Some("0123456789abcdef0123456789abcdef".to_owned()),
            "PLUGINS" => Some(plugins.to_owned()),
            _ => None,
        })
        .unwrap()
    }

    #[test]
    fn the_report_separates_what_is_compiled_in_from_what_is_on() {
        let (_, report) = wire_plugins(&config("tracing")).unwrap();

        assert_eq!(report.enabled, ["tracing"]);
        assert_eq!(report.available, ["tracing", "in-process"]);
    }

    /// Neither shipped plugin is a user-facing feature, so a default build advertises none.
    #[test]
    fn publisher_only_plugins_advertise_no_capabilities() {
        let (_, report) = wire_plugins(&config("tracing,in-process")).unwrap();

        assert_eq!(report.enabled, ["tracing", "in-process"]);
        assert!(report.capabilities.is_empty());
    }

    #[test]
    fn disabling_publishing_still_reports_what_is_available() {
        let (_, report) = wire_plugins(&config("")).unwrap();

        assert!(report.enabled.is_empty());
        assert_eq!(report.available, ["tracing", "in-process"]);
    }

    #[test]
    fn a_typo_in_plugins_refuses_to_start_rather_than_silently_disabling() {
        let Err(err) = wire_plugins(&config("tracing,tracnig")) else {
            panic!("an unknown plugin name must not start the process");
        };

        assert!(matches!(
            err,
            BootstrapError::Plugin(PluginError::Unknown(_))
        ));
    }
}
