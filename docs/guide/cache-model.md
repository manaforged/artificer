# Cache model

A unit is one crate compiled with one set of inputs. A unit key covers
what the compile actually reads: compiler identity, compile options,
features, profile inputs, lint flags, dependency artifacts, build-script
output, relevant environment values, and package content. Source
symlinks are followed and their targets included. Registry checkouts are
keyed by package identity instead of a tree scan.

The checkout path is not keyed directly. An input that contains the path,
such as `CARGO_MANIFEST_DIR` read at compile time, an absolute path in
`RUSTFLAGS`, or build-script output that records the path, changes that
unit's key and the keys of the units that depend on it. rustc gets the key as
`-C metadata`, and `--remap-path-prefix` maps the package root to `.`.
Two worktrees of the same code at different paths produce the same unit.
Workspace members are cached the same way as dependencies, and
proc-macros are cached like any other crate.

Cargo configuration that changes a compile is part of the key. Variables
from `[env]` (with `force` and `relative`) are set for rustc and build
scripts and keyed with workspace and store paths replaced by
placeholders. A `linker` from `[target.<host>]` or a matching
`[target.cfg(..)]` is passed to rustc as `-C linker` and keyed. A `runner`
does not change compiled output and is not keyed. `[patch]` changes only
dependency resolution, which Artificer reads from Cargo, so the patched
source is already part of each package identity.

A build script runs once per key. Its `OUT_DIR` is stored with the unit,
and `rerun-if-changed` and `rerun-if-env-changed` are honored.

Before reusing a unit, Artificer validates rustc's dependency records
against current file contents and environment values. This includes
files outside the package. An unset variable and an empty value are
distinct inputs.

An operating-system lock per digest stops two processes from publishing
the same miss twice. A unit is published only after rustc succeeds, and a
published unit is never modified. If a build is killed, the operating
system releases its lock and no partial unit remains. The next build
compiles that crate. On Unix one FIFO jobserver shares the host core
budget across concurrent builds. Within one build, rustc's extra threads
draw from a second pool that the build owns, as with Cargo, so a killed
rustc cannot drain the shared one. `CARGO_BUILD_JOBS` and `ARTIFICER_JOBS`
cap workers within each process. Windows has no cross-process pool. Each
process schedules with its own workers.

Default store: the OS cache directory (`~/Library/Caches/artificer`,
`$XDG_CACHE_HOME/artificer`, or `%LOCALAPPDATA%\artificer`). Set
`ARTIFICER_HOME` to move it. If the store is not writable, Cargo runs
unchanged.

Default cap: 15% of the store volume, with a minimum of 8 GiB. Eviction
removes least-recently-used units first. Daily garbage collection
removes units unused for 30 days. Set `ARTIFICER_STORE_CAP_GB` to lower
the cap. Larger values are clamped to the default limit.

Workspace crates change with each edit, so each edit publishes a new
unit. Artificer keeps the newest unit for each workspace crate, target
kind, and settings, per checkout path, and evicts the unit it replaces
when that unit is not in use. Registry and git dependencies are not
evicted this way.

Optional compile modes (`artificer mods`) add rmeta-only checks,
line-table debug info for dependencies, incremental cleanup, and the
local daemon. See the [command and configuration
reference](reference.md).
