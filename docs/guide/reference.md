# Command and configuration reference

## Command dispatch

The cargo shim keeps the original argument vector. If Artificer does not support an
invocation, it starts the configured Cargo executable with those arguments.
The direct artificer executable reports the reason and exits with status 2.

| Command | Artificer path |
| --- | --- |
| check | Package or workspace checking, including default binary targets |
| build | Package or workspace linking |
| run | One binary or one named example, with Cargo-compatible child arguments |
| test | Unit, integration, binary, and documentation tests. Test binaries and doctests run at the same time, up to the job limit (`ARTIFICER_JOBS` or `CARGO_BUILD_JOBS`). Cargo runs them one at a time |
| artificer warm | Workspace check that populates the store |
| artificer clean | Evict expired units, enforce the store cap, and delete `target/<profile>/{incremental,deps,.fingerprint,build}` |
| clippy | While caching is enabled, cargo-clippy runs with the shim as `CARGO`; otherwise Cargo runs it |
| nextest | While caching is enabled, cargo-nextest runs with the shim as `CARGO`; otherwise Cargo runs it |
| all other Cargo commands | Configured Cargo executable |

### Supported options

The supported build commands accept the applicable subset of:

- -p and --package;
- --workspace;
- --features, --all-features, and --no-default-features;
- --locked, --offline, and --frozen;
- --release and the dev or release profile names;
- --message-format=human, =json, and =json-render-diagnostics;
- -j/--jobs N, -q/--quiet, -v/--verbose, and --color auto|always|never;
- --tests and --all-targets for check;
- --bin and --example for run;
- --no-run, --lib, --doc, and --test for test;
- a run argument tail;
- one test filter and test-harness arguments after --.

Artificer forwards the complete command when it sees an unsupported
input. Fallback cases include:

- `--target` and target-specific environment rustflags;
- bench and doc commands;
- custom profiles;
- build target selection outside run;
- Cargo aliases;
- --keep-going, and a --color value outside auto, always, and never;
- `[profile.*]` keys Artificer does not apply, unknown lint levels, and
  `[profile]` tables in `.cargo/config.toml`;
- `build.rustc`, `build.target`, and target linker/runner settings in
  `.cargo/config.toml`;
- Cargo-owned environment Artificer would otherwise ignore:
  `CARGO_PROFILE_*`, `CARGO_UNSTABLE_*`, `CARGO_TARGET_*` (except
  `CARGO_TARGET_DIR`), and the `CARGO_BUILD_*` settings that change the
  build;
- a feature probe (the step that asks Cargo which features are enabled)
  that cannot resolve the per-invocation feature set.

On fallback, the shim runs the configured Cargo with the original
arguments. Run the `artificer` binary
directly to see the reason.

## Maintenance commands

| Command | Effect |
| --- | --- |
| artificer stat [--json] | Show cache size, entry counts, hit and miss totals, the last build, fallback count, and modes; `--json` prints one object |
| artificer why-fallback [--limit N] | Show fallback reasons, most frequent first |
| artificer why-miss CRATE | Show what changed between the last two key records of one crate |
| artificer doctor | Check the mode, shim PATH precedence, real Cargo, store path, jobserver, store, rustc, fallbacks, daemon, and rust-analyzer |
| artificer warm | Compile the current workspace into the shared store |
| artificer clean | Evict expired units, enforce the store cap, and delete `target/<profile>/{incremental,deps,.fingerprint,build}` |
| artificer serve | Start the local daemon in the foreground |
| artificer serve stop | Stop the local daemon and remove its control files |
| artificer env | Print the shell command that puts the shim first on PATH |
| artificer export DIR [--days N] [--max-gb N] | Copy units used in the last N days (default 7), newest first, under the size cap |
| artificer import DIR | Add missing units from an exported directory |
| artificer enable / disable | Persist caching state for the selected store |
| artificer install [--no-modify-path] | Put the launchers and recorded Cargo path in place and add the shim to PATH (shell profiles on macOS and Linux, the user PATH on Windows); `--no-modify-path` skips this |
| artificer uninstall [--purge] | Remove the launchers, recorded Cargo path, and the PATH entries install added; run `cargo uninstall artificer-build` when Cargo installed Artificer; `--purge` also deletes the cache |
| artificer mods | List compile modes |
| artificer help COMMAND, artificer COMMAND --help | Print the usage and options of one command |
| artificer mods on NAME | Enable one compile mode |
| artificer mods off NAME | Disable one compile mode |

`artificer clean` evicts expired units, enforces the store cap, and
deletes `target/<profile>/{incremental,deps,.fingerprint,build}`. It does
not delete the store. The sweep mode removes only the
`incremental/` directories, automatically, during handled builds.

## Exit codes

| Code | Meaning |
| --- | --- |
| 0 | The command succeeded |
| 1 | `artificer doctor` found a row that reads `BAD` |
| 2 | The direct `artificer` binary does not support the invocation, or the arguments are wrong. The shim falls back only when it declines an invocation; a child exit code of 2 is returned unchanged. |
| 101 | An error stopped the command, including a compile failure |
| other | `run` and `test` return the exit code of the program or test harness |

## Compile modes

The configuration file is ARTIFICER_HOME/mods.toml. Use artificer mods instead of
editing it. An unknown name, malformed line, or invalid Boolean is an error.

| Mode | Fresh default | Effect |
| --- | --- | --- |
| enabled | on | Cache supported invocations; off sends shim commands directly to Cargo |
| sweep | off | Remove Cargo incremental directories during handled builds |
| cranelift | off | Use Cranelift when rustc accepts it |
| rmeta | on | Emit metadata-only artifacts for check where possible |
| slim | off | Use line-table debug info and disable embedded bitcode |
| linker | off | Probe for mold, wild, ld64.mold, or lld |
| meta-cache | on | Cache Cargo metadata outside the workspace target directory |
| threads | off | Use rustc parallel frontend threads when rustc accepts them |
| serve | off | Send handled builds to the local daemon and start it on demand. Not available on Windows |

## Environment variables

| Variable | Meaning |
| --- | --- |
| ARTIFICER_DISABLED | Set to any value, including `0` or empty, to bypass caching for shim commands, before metadata or compiler probes |
| ARTIFICER_HOME | Store and control directory; default is the OS cache directory |
| ARTIFICER_REAL_CARGO | System Cargo executable or Rustup proxy used for metadata and shim fallback |
| ARTIFICER_NOSERVE | Force the in-process build path |
| ARTIFICER_NO_TREE | Skip the per-invocation feature probe; every handled command falls back to Cargo |
| ARTIFICER_STORE_CAP_GB | Store size limit in GiB. Unset, the cap is 15% of the volume (at least 8 GiB). A value above that share is clamped to it. |
| ARTIFICER_JOBS | Maximum Artificer compile workers in one process |
| CARGO_BUILD_JOBS | Cargo's job cap; used when ARTIFICER_JOBS is unset |
| ARTIFICER_CODEGEN=llvm or off | Disable automatic Cranelift selection when its mode is on |
| ARTIFICER_LINKER=off or default | Disable automatic linker selection |
| ARTIFICER_LINKER=PATH | Use an explicit linker when the linker mode is on |
| ARTIFICER_THREADS=off | Disable rustc frontend threads when their mode is on |
| ARTIFICER_TIMING | Print Artificer phase timings |
| ARTIFICER_TRACE | Print rustc commands |
| ARTIFICER_DEBUG_KEY=CRATE | Print key inputs for one crate |
| ARTIFICER_DEBUG_SEL | Print feature-resolution selection |
| ARTIFICER_PASSES | Set to any value to pass `-Ztime-passes` to rustc |
| ARTIFICER_SHIM | Set to any value to stop the direct `artificer` binary from printing the fallback reason |

Artificer honors RUSTC, RUSTFLAGS, CARGO_ENCODED_RUSTFLAGS, RUSTDOC,
RUSTDOCFLAGS for doctests, RUSTC_WRAPPER and RUSTC_WORKSPACE_WRAPPER
(environment or `build.rustc-wrapper` / `build.rustc-workspace-wrapper`
in config, nested the way Cargo nests them), Cargo feature and profile
inputs, and relevant compiler/build-script environment variables.
Per-target CARGO_TARGET_TRIPLE_RUSTFLAGS variables cause full Cargo
fallback.

An explicitly empty RUSTFLAGS clears config rustflags, matching Cargo:
set and unset are different states.

## Files and cleanup

The default layout below is relative to ARTIFICER_HOME:

| Path | Contents |
| --- | --- |
| units/LAYOUT/u-DIGEST | Immutable compile units (LAYOUT is the store format version) |
| locks/LAYOUT/ | Operating-system locks for unit writes and shared probes |
| leases/LAYOUT/ | Read locks held until a build finishes using each unit |
| cargo-meta/ | Redirected Cargo metadata target and cache |
| keys/ | Last two unit-key traces per local crate |
| builds.jsonl | Last 200 handled commands: hits, misses, duration, fallback reason |
| mods.toml | Compile-mode configuration |
| serve.port, serve.token, serve.pid | Optional daemon control files |

Unit use refreshes the unit marker. Daily garbage collection removes units
unused for 30 days, in the current store layout and in older ones, key traces older than 30 days, and expired metadata
entries. Every hour, a build also enforces the store limit by evicting the
oldest units without active readers or writers first. Both passes run on a background thread beside
a build, not before it.

Concurrent commands share one `cargo metadata` probe and one `cargo tree`
probe per key, and compile one unit per digest: the second process waits
for the first, then reads the result. Concurrent builds on Unix also
share one jobserver.

## Uninstall

Run `artificer uninstall`. It stops the daemon, removes the launchers and
the recorded Cargo path, and removes the PATH entries that `artificer
install` added: the shell profile lines on macOS and Linux, and the user
PATH entry on Windows. When Cargo installed Artificer, it also
runs `cargo uninstall artificer-build` on macOS and Linux, and prints that
command on Windows. The cache remains unless you pass
`--purge`. Start a new shell or, on Unix, run `hash -r` to refresh command
lookup.

On Windows, `scripts/install.ps1 -Uninstall` removes the running binary
after it exits.
