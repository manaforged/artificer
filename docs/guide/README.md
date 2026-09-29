# Artificer user guide

Artificer shares Rust compile work across every checkout on one
machine. It installs as a `cargo` shim: supported commands compile through
a content-addressed unit store, so a second clone, a fresh worktree, or a
wiped `target/` reuses units instead of recompiling them.

An unchanged build can reuse compiler outputs. Reuse depends on source
content, compiler settings, dependencies, and environment values. Paths used
by the compiled program can make otherwise identical checkouts differ.
Unsupported invocations run through Cargo with their original arguments.
The store uses local files; the optional daemon listens only on loopback.

## Reading order

1. [Install](install.md): install, PATH, uninstall, editors, and CI
   shells.
2. [Safety contract](safety-contract.md): when a command falls back to
   Cargo and how you see it.
3. [Cache model](cache-model.md): what a unit key covers, where the store
   lives, and how eviction works.
4. [CI](ci.md): move the store between runs with `export` and `import`.
5. [Measurements](measurements.md): the timed cases and how to reproduce
   them.
6. [Reference](reference.md): every command, supported option, compile
   mode, environment variable, and store path.
7. [Architecture](architecture.md): how it works inside.

Scripts and tools read the [API reference](../api.md).
