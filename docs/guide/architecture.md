# Artificer architecture

Artificer has two entry modes. The artificer executable exposes maintenance commands
and the supported build interface. The same executable, copied as cargo,
acts as a shim.

~~~text
command
  |
  +-- unsupported shape --------------------> configured Cargo
  |
  +-- supported shape
        |
        +-- Cargo metadata + manifests + config
        |
        +-- dependency schedule
        |
        +-- content key
              |
              +-- hit ----------------------> immutable unit
              |
              +-- miss -> rustc -> publish -> immutable unit
~~~

## Module map

| Module | Owns |
| --- | --- |
| src/action.rs | One content-addressed action: key streaming, store slot, lock, finish policy |
| src/cli.rs | Argument grammar: parse exactly, or return the fallback reason |
| src/main.rs | Process wiring: shim detection, dispatch, exit codes, maintenance commands |
| src/gate.rs | The dispatch boundary: every input supported exactly, or Cargo runs the command |
| src/build.rs | Build entry points: plan one command, compile the graph, run tests |
| src/features.rs | Per-invocation feature resolution (`cargo tree` probe and narrowing) |
| src/settings.rs | Resolved build configuration: toolchain, flags, profile, wrappers, lints |
| src/session.rs | Per-invocation state: settings plus artifact, script, and hash caches |
| src/compile.rs | Unit pipeline: compile a package, a bin, a test harness, or a check unit |
| src/unit_key.rs | Unit identity and dependency-artifact validation |
| src/inputs.rs | Validation of rustc file and environment dependency records |
| src/invoke.rs | Shared rustc command assembly and execution |
| src/artifact.rs | Artifact naming, location, and delivery into `target/` |
| src/cargo.rs | `cargo metadata` model and graph queries |
| src/config.rs | `.cargo/config.toml` discovery, merge, and cfg matching |
| src/manifest.rs | `[lints]` and `[profile.*]` tables metadata omits |
| src/store.rs | Immutable unit store: stage, rename, restore, gc, locks |
| src/script.rs | Build-script compile, run, and `OUT_DIR` restore |
| src/schedule.rs | Dependency-ordered compile workers |
| src/jobs.rs | Shared FIFO jobserver (Unix) |
| src/key.rs | Package-content hashing and compile-time env discovery |
| src/mods.rs | Optional compile modes |
| src/serve.rs | Optional loopback daemon |
| src/sweep.rs | Target-directory cleanup for sweep mode and `artificer clean` |
| src/flags.rs | rustc capability probes (frontend threads and fork flags) |
| src/platform.rs | Host probes: pid liveness, hostname, path spelling |
| src/out.rs | Output routing: human lines and cargo JSON |
| src/home.rs | Store location and the PATH snippet |
| src/maintenance.rs | stat, clean, gc, and doctor |

## Dispatch boundary

src/cli.rs parses only command shapes Artificer supports. src/gate.rs checks
the whole invocation before any compile starts: profile keys, lint levels,
config settings, Cargo-owned environment, target-rustflags matchers, and
the per-invocation feature probe. If any input cannot be represented
exactly, the complete invocation falls back; there is no approximate path.
Artificer does not split one command between its planner and Cargo.

Dispatch returns either a completed command or a fallback decision. The shim
starts Cargo only for the latter. Child exit codes, including 2, are returned
without retrying the command.

## Build planning

src/build.rs reads Cargo metadata and resolves the package graph.
src/settings.rs resolves one profile into the exact rustc configuration.
src/features.rs narrows the graph to the per-invocation feature set.
src/session.rs carries the shared caches. src/schedule.rs compiles ready
graph nodes concurrently. A library starts once its dependencies have
written their metadata, as Cargo's pipelining does; a unit that links
waits for every dependency to finish. A package's build script is its own
unit: it starts once its build-dependencies are built and the build
scripts of its `links` dependencies have run. A package's tests, checked
targets, and extra binaries compile at the same time through
`schedule::fan_out`, the pool that also runs test binaries.

src/compile.rs builds rustc and rustdoc commands through src/invoke.rs.
It stores each unit's digest in the session so each downstream key
names the exact upstream unit.

## Unit keys

src/key.rs hashes local package trees, excluding build output, version
control data, dependency vendor caches such as node_modules, and the
Artificer store. It hashes file paths and bytes in a stable order. It
follows valid source symlinks, includes each link target in the key, and
rejects directory symlink cycles.

src/unit_key/scope.rs narrows the tree for each unit. A unit leaves out
the files of the package's other integration tests, examples, benches,
and build script. A library or binary also leaves out the `tests`,
`examples`, and `benches` directories. A file read from outside the
narrowed tree is still checked against rustc's dependency records
before reuse.

src/unit_key.rs combines the package-content digest with package
identity, compiler identity, target kind, crate types, enabled features,
profile and lint flags, relevant environment variables, build-script
output, and dependency unit digests.

Registry package identity replaces a tree scan only when the manifest is
inside Cargo's registry directory. Vendored and source-replaced packages
are hashed as local content.

## Store

src/store.rs owns the store format. Units live below
ARTIFICER_HOME/units/LAYOUT/u-DIGEST, where LAYOUT is the version prefix
from src/store.rs (v5). A complete unit has an ok marker and an out
directory.

rustc writes registry and git crates directly into the unit directory under an
operating-system file lock. Workspace crates compile in a stable directory under
`target/<profile>/artificer/`, which keeps rustc's incremental cache valid, and
their outputs are then linked into the unit directory.
The ok marker is written only after successful compilation and dependency recording.
Builds retain read locks until their compilers and executables finish using the units.
Replacement and garbage collection require an exclusive lock. A conflicting build
falls back to Cargo instead of replacing files that another build is reading.
Import and export publish complete copies through a staging directory and rename.

The store refreshes the ok marker on use. Daily garbage collection removes
units older than 30 days. An hourly pass evicts least-recently-used units when
the store exceeds its byte limit. Units with active readers or writers are skipped. Units of other store-layout versions stay while any version uses them; daily garbage collection removes those unused for 30 days.

Change src/store.rs LAYOUT when an existing digest would refer to a different
set of build inputs or output semantics.

## Process coordination

On Unix, src/jobs.rs maintains a shared FIFO jobserver. Each process holds a shared lifetime lock.
The pool is refilled only when no existing process holds that lock. Each build session also
owns a private FIFO pool under `jobservers/` in the store; a worker takes one token for each
rustc, rustc's extra threads draw the rest, and the pool is removed when the session ends. Windows has no
cross-process pool: each Artificer process schedules its own workers.

Cross-process work is deduplicated per key: one `cargo metadata` resolve,
one `cargo tree` probe, and one unit compile per digest. The second
process waits on the key's lock and reads the first one's result. Daily
store maintenance runs on a background thread beside a build.

The optional daemon in src/serve.rs listens on an ephemeral 127.0.0.1 port.
It authenticates each newline-delimited JSON request with a random token.
Control files are private on Unix. Requests are bounded to 1 MiB, and the
daemon accepts at most 64 concurrent connections.

The daemon records a digest of its environment. Clients with different
environments or an explicit `-j` limit build in-process. Both paths call
the same build functions.

## Compatibility evidence

Unit tests cover parsers, keys, scheduling, store publication, and daemon
limits. Integration suites cover CLI behavior and graph compilation.
tests/parity.rs runs fixtures with Artificer and Cargo and compares exit status,
artifacts, or program output as appropriate.

Parity coverage defines the tested compatibility surface. Unsupported or
uncertain shapes belong to Cargo at the dispatch boundary.
