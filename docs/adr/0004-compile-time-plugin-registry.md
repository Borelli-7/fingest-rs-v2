# 0004 — Compile-time plugin registry, no dynamic loading

## Status

Accepted.

## Context

Event publishing needs to be extensible: today events go to the log, tomorrow to a broker,
and a deployment should be able to choose without a source change. "Plugin" usually implies
dynamic loading — a shared object discovered at runtime and `dlopen`ed.

Rust has no stable ABI. Two crates compiled by different compiler versions, or with
different feature flags, have no guaranteed-compatible representation for a `Vec`, a trait
object, or a `Result`. A `dlopen`-style boundary must therefore be `extern "C"`, which means
every value crossing it is reduced to C-representable types, manually marshalled, and
unsafe on both sides.

The workspace forbids unsafe code outright (`unsafe_code = "forbid"` in the root
`[workspace.lints.rust]`). Reintroducing it at the extension point — the one place least
covered by tests — to gain runtime loading of plugins that are all developed in this
repository would be a poor trade.

## Decision

Plugins are compiled into the binary and selected by name at startup.

`fingest-plugins` defines the `Plugin` trait (`name()`, `register(&mut Registry)`), a
`Registry` that plugins contribute `Arc<dyn EventPublisher>` to, and a `PluginHost` that
holds every compiled-in plugin. `PluginHost::build(&names)` instantiates only the named
subset and returns a `Registry`; `Registry::into_publisher()` collapses the contributions
into a `FanOutPublisher`.

`fingest-bootstrap` registers the two plugins that exist — `TracingPlugin` (`"tracing"`) and
`InProcessPlugin` (`"in-process"`) — and passes `config.plugins`, parsed from the `PLUGINS`
environment variable. An unknown name is `PluginError::Unknown` and stops startup rather
than being ignored. An empty `PLUGINS` yields a registry with no publishers, which disables
publishing.

## Consequences

- No unsafe code, no ABI concerns, and a plugin's ports are checked by the compiler like any
  other code.
- Misconfiguration fails loudly at startup instead of silently dropping events.
- Adding a plugin requires a rebuild and a release. Given that all plugins live in this
  workspace, this changes nothing in practice; it would matter if third parties needed to
  extend a running deployment, which is not a requirement.
- `FanOutPublisher::publish` returns on the first error, so the batch is not partially
  marked delivered — but a slow or failing publisher stalls every other publisher behind it.
  Per-publisher isolation would need its own decision record.
- The registry only has one extension point today (event publishing). The `Plugin` trait is
  shaped so a second kind of contribution can be added to `Registry` without changing the
  trait's callers.
