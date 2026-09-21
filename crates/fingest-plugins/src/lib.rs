//! Compile-time plugin registry.
//!
//! Plugins are linked into the binary and selected by name from configuration. There is no
//! dynamic loading: Rust has no stable ABI, so a `dlopen`-style boundary would have to be
//! C-shaped and would discard exactly the type safety this rewrite exists for (ADR-004).
//!
//! The single extension point today is event publishing. A plugin contributes an
//! [`EventPublisher`]; the registry fans out to every contributor.

use std::sync::Arc;

use async_trait::async_trait;
use fingest_kernel::{EventEnvelope, EventPublisher, PortError};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PluginError {
    #[error("Unknown plugin: {0}")]
    Unknown(String),

    #[error("Plugin registered twice: {0}")]
    Duplicate(String),
}

pub trait Plugin: Send + Sync {
    fn name(&self) -> &'static str;

    /// Feature names this plugin unlocks for clients.
    ///
    /// Declared rather than discovered, so the set is knowable without running
    /// [`Plugin::register`]. A plugin that only contributes a publisher has none: a
    /// logging backend is not a user-facing feature.
    fn capabilities(&self) -> &[&'static str] {
        &[]
    }

    fn register(&self, registry: &mut Registry);
}

/// What plugins contribute to.
#[derive(Default)]
pub struct Registry {
    publishers: Vec<Arc<dyn EventPublisher>>,
    capabilities: Vec<String>,
}

impl Registry {
    pub fn add_publisher(&mut self, publisher: Arc<dyn EventPublisher>) {
        self.publishers.push(publisher);
    }

    pub fn publisher_count(&self) -> usize {
        self.publishers.len()
    }

    /// Every capability declared by the enabled plugins, in activation order, deduplicated.
    pub fn capabilities(&self) -> &[String] {
        &self.capabilities
    }

    fn add_capability(&mut self, capability: &str) {
        if !self.capabilities.iter().any(|c| c == capability) {
            self.capabilities.push(capability.to_owned());
        }
    }

    /// Collapses the contributions into one publisher.
    pub fn into_publisher(self) -> Arc<dyn EventPublisher> {
        Arc::new(FanOutPublisher {
            publishers: self.publishers,
        })
    }
}

/// Trait objects cannot derive `Debug`; the count is the useful part anyway.
impl std::fmt::Debug for Registry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registry")
            .field("publishers", &self.publishers.len())
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

/// Publishes to every contributor.
///
/// The first failure aborts the batch and propagates, so the relay leaves the rows pending
/// and retries rather than partially marking them delivered.
struct FanOutPublisher {
    publishers: Vec<Arc<dyn EventPublisher>>,
}

#[async_trait]
impl EventPublisher for FanOutPublisher {
    async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError> {
        for publisher in &self.publishers {
            publisher.publish(events).await?;
        }
        Ok(())
    }
}

/// Holds every compiled-in plugin and activates the subset named in configuration.
#[derive(Default)]
pub struct PluginHost {
    plugins: Vec<Box<dyn Plugin>>,
}

impl PluginHost {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, plugin: Box<dyn Plugin>) -> Result<(), PluginError> {
        if self.plugins.iter().any(|p| p.name() == plugin.name()) {
            return Err(PluginError::Duplicate(plugin.name().to_owned()));
        }
        self.plugins.push(plugin);
        Ok(())
    }

    pub fn available(&self) -> Vec<&'static str> {
        self.plugins.iter().map(|p| p.name()).collect()
    }

    /// Activates `enabled`, in the order given. An unrecognised name is an error rather
    /// than a silent no-op, so a typo in configuration cannot quietly disable auditing.
    pub fn build(&self, enabled: &[String]) -> Result<Registry, PluginError> {
        let mut registry = Registry::default();

        for name in enabled {
            let plugin = self
                .plugins
                .iter()
                .find(|p| p.name() == name)
                .ok_or_else(|| PluginError::Unknown(name.clone()))?;

            plugin.register(&mut registry);
            for capability in plugin.capabilities() {
                registry.add_capability(capability);
            }
            tracing::info!(plugin = plugin.name(), "plugin enabled");
        }

        Ok(registry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct Counting {
        seen: Mutex<usize>,
    }

    #[async_trait]
    impl EventPublisher for Counting {
        async fn publish(&self, events: &[EventEnvelope]) -> Result<(), PortError> {
            *self.seen.lock().unwrap() += events.len();
            Ok(())
        }
    }

    struct Exploding;

    #[async_trait]
    impl EventPublisher for Exploding {
        async fn publish(&self, _: &[EventEnvelope]) -> Result<(), PortError> {
            Err(PortError::Unavailable("boom".into()))
        }
    }

    struct CountingPlugin(&'static str, Arc<Counting>);

    impl Plugin for CountingPlugin {
        fn name(&self) -> &'static str {
            self.0
        }
        fn register(&self, registry: &mut Registry) {
            registry.add_publisher(self.1.clone());
        }
    }

    struct ExplodingPlugin;

    impl Plugin for ExplodingPlugin {
        fn name(&self) -> &'static str {
            "exploding"
        }
        fn register(&self, registry: &mut Registry) {
            registry.add_publisher(Arc::new(Exploding));
        }
    }

    struct CapablePlugin(&'static str, &'static [&'static str]);

    impl Plugin for CapablePlugin {
        fn name(&self) -> &'static str {
            self.0
        }
        fn capabilities(&self) -> &[&'static str] {
            self.1
        }
        fn register(&self, _: &mut Registry) {}
    }

    fn envelope() -> EventEnvelope {
        EventEnvelope::new(
            &fingest_kernel::DomainEvent::AccountRegistered {
                login: "bob".into(),
                admin: false,
            },
            chrono_now(),
        )
        .unwrap()
    }

    fn chrono_now() -> chrono::DateTime<chrono::Utc> {
        chrono::Utc::now()
    }

    #[test]
    fn an_unknown_plugin_name_is_rejected() {
        let host = PluginHost::new();

        let err = host.build(&["nope".to_owned()]).unwrap_err();

        assert_eq!(err, PluginError::Unknown("nope".into()));
    }

    #[test]
    fn registering_the_same_name_twice_is_rejected() {
        let mut host = PluginHost::new();
        host.register(Box::new(CountingPlugin("a", Arc::default())))
            .unwrap();

        let err = host
            .register(Box::new(CountingPlugin("a", Arc::default())))
            .unwrap_err();

        assert_eq!(err, PluginError::Duplicate("a".into()));
    }

    #[test]
    fn only_enabled_plugins_contribute() {
        let mut host = PluginHost::new();
        host.register(Box::new(CountingPlugin("a", Arc::default())))
            .unwrap();
        host.register(Box::new(CountingPlugin("b", Arc::default())))
            .unwrap();

        let registry = host.build(&["a".to_owned()]).unwrap();

        assert_eq!(registry.publisher_count(), 1);
        assert_eq!(host.available(), vec!["a", "b"]);
    }

    #[test]
    fn a_publisher_only_plugin_declares_no_capabilities() {
        let mut host = PluginHost::new();
        host.register(Box::new(CountingPlugin("a", Arc::default())))
            .unwrap();

        let registry = host.build(&["a".to_owned()]).unwrap();

        assert!(registry.capabilities().is_empty());
    }

    #[test]
    fn only_enabled_plugins_contribute_capabilities() {
        let mut host = PluginHost::new();
        host.register(Box::new(CapablePlugin("a", &["budget-forecast"])))
            .unwrap();
        host.register(Box::new(CapablePlugin("b", &["audit-trail"])))
            .unwrap();

        let registry = host.build(&["a".to_owned()]).unwrap();

        assert_eq!(registry.capabilities(), ["budget-forecast"]);
    }

    #[test]
    fn capabilities_keep_activation_order_and_deduplicate() {
        let mut host = PluginHost::new();
        host.register(Box::new(CapablePlugin("a", &["shared", "only-a"])))
            .unwrap();
        host.register(Box::new(CapablePlugin("b", &["shared", "only-b"])))
            .unwrap();

        let registry = host.build(&["a".to_owned(), "b".to_owned()]).unwrap();

        assert_eq!(registry.capabilities(), ["shared", "only-a", "only-b"]);
    }

    #[tokio::test]
    async fn every_enabled_publisher_receives_the_batch() {
        let first = Arc::new(Counting::default());
        let second = Arc::new(Counting::default());
        let mut host = PluginHost::new();
        host.register(Box::new(CountingPlugin("a", first.clone())))
            .unwrap();
        host.register(Box::new(CountingPlugin("b", second.clone())))
            .unwrap();

        let publisher = host
            .build(&["a".to_owned(), "b".to_owned()])
            .unwrap()
            .into_publisher();

        publisher.publish(&[envelope(), envelope()]).await.unwrap();

        assert_eq!(*first.seen.lock().unwrap(), 2);
        assert_eq!(*second.seen.lock().unwrap(), 2);
    }

    #[tokio::test]
    async fn one_failing_publisher_fails_the_batch() {
        let mut host = PluginHost::new();
        host.register(Box::new(ExplodingPlugin)).unwrap();

        let publisher = host
            .build(&["exploding".to_owned()])
            .unwrap()
            .into_publisher();

        assert!(publisher.publish(&[envelope()]).await.is_err());
    }

    #[tokio::test]
    async fn no_plugins_means_publishing_is_a_no_op() {
        let publisher = PluginHost::new().build(&[]).unwrap().into_publisher();

        assert!(publisher.publish(&[envelope()]).await.is_ok());
    }
}
