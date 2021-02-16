# drbdd

`drbdd` is a daemon consisting of a core that does DRBD events processing and plugins that can react on
changes in a DRBD resource.

# Plugins

| Plugin                               | Purpose                        |
| ------------------------------------ | ------------------------------ |
| [debugger](./src/plugin/debugger.md) | Demo that prints state changes |
| [promoter](./src/plugin/promoter.md) | Simple HA for resources        |
| [umh](./src/plugin/umh.md)           | User mode helper               |

## Implementation
- [x] debugger
- [x] promoter
- [ ] umh

# Configuration
This daemon is configured via a configuration file. The only command line option allowed is the path to the
configuration. The default location for the config is `/etc/drbdd.toml`. The repository contains an example
[drbdd.toml](./example/drbdd.toml).

# Building

This is a Rust application. If you have a Rust toolchain and `cargo` installed (via distribution packages or
[rustup](https://rustup.sh)) you can build it via:

```
cargo build
```

## Packages

`rpm` and `deb` packages can be built in containers, which requires Docker. This repository contains a self
documenting `Makefile` (just execute `make help`). Building a Debian package looks like this:

```
make debcontainer # only execute this once
make deb # as often as needed
```

# Architecture

## Core

The core consists of 2 threads. The first one is responsible for actual `drbdsetup events2` processing. It
sends update event structs on a channel to the main thread.

The main thread keeps track of the overall DRBD resource state by applying these updates to an internal map of
resource structs. Think of these as the output of `drbdsetup status --json`. The second purpose is to generate
`PluginUpdate` enums if important properties of a resource changed. For example one variant of the
`PluginUpdate` is `ResourceRole`, that is generated if the role of a resource changed. These variants follow
the same structure: They contain the event type, information that identifies the actual DRBD object (resource
name, peer ID, volume ID,...) and the `old` and `new` states. These old/new states contain the rest of
the relevant information within this event (e.g., the `may_promote`, and `promotion_score`). Think of `old`
and `new` structs as easy to consume diffs. A `PluginUpdate` also contains the current, complete state of the
resource.

## Plugins

Plugins are maintained in this repository. Every plugin is started as its own thread by the `core`.
Communication is done via channels where plugins only consume information.

The core expose a `PluginUpdate` channel, the plugin decides if it wants to use the diffs from `old` and
`new`, and/or the complete current state from `resource`.

# Current implementation considerations
Currently plugins have to filter their `PluginUpdate` stream by themselves. This keeps the core simple, but
allowing some kind of filtered subscription could make sense. If we don't do that, and keep the "all plugins
get all events" semantic, we could switch to a broadcast channel, there are some crates out there.
