# Artificer API reference

The supported integration surface is the `artificer` command line, the
`cargo` shim, exit codes, and `artificer stat --json`.

The Rust library supports the binary and its tests; it is not a supported
embedding API. Store files are private implementation details. Use
`artificer export` and `artificer import` to transfer cached units.
Human-readable diagnostics can change; use `artificer stat --json` for tooling.

## Core commands

One executable has two entry modes. Named `cargo`, it is the shim: it
handles `check`, `build`, `test`, and `run`, and sends everything else to
real Cargo. Named `artificer`, it accepts the commands below.

| Command | Effect |
| --- | --- |
| `artificer check` | Check a package or workspace |
| `artificer build` | Build a package or workspace |
| `artificer run` | Build and run a binary or example |
| `artificer test` | Build and run tests |
| `artificer warm` | Populate the store for a workspace |
| `artificer clean` | Evict expired units, enforce the store cap, and delete `target/<profile>/{incremental,deps,.fingerprint,build}` |
| `artificer serve` | Run the local daemon in the foreground |
| `artificer serve stop` | Stop the local daemon |
| `artificer stat [--json]` | Show store and mode status |
| `artificer doctor` | Check the mode, shim PATH precedence, real Cargo, store, rustc, fallbacks, daemon, and rust-analyzer |
| `artificer why-miss CRATE` | Show what changed between the last two key records of a crate |
| `artificer why-fallback [--limit N]` | Show fallback reasons, most frequent first; `N` defaults to 5 |
| `artificer profile [ID] [--json] [--trace FILE] [--html FILE]` | Show where a build spent its time; `ID` is a profile ID or a unique prefix, and defaults to the newest build |
| `artificer profile list [--limit N] [--json]` | List recorded builds, newest first; `N` defaults to 20 |
| `artificer profile diff BASE HEAD [--json]` | Compare two recorded builds |
| `artificer env` | Print the command that puts the shim first on PATH |
| `artificer enable`, `artificer disable` | Persistently enable caching or send shim commands directly to Cargo for the selected store |
| `artificer install [--remote LOCATION]` | Place the launchers, the recorded Cargo path, and the PATH file; `--remote` also sets the remote store and starts the first pull in the background |
| `artificer uninstall` | Remove the launchers and profile PATH lines; keep the cache unless `--purge` |
| `artificer export DIR [--days N] [--max-gb N]` | Copy recently used units to a directory |
| `artificer import DIR` | Add missing units from an exported directory |
| `artificer remote` | Show the remote store |
| `artificer remote set LOCATION` | Set the remote store: `HOST:/ABSOLUTE/PATH` over SSH, or an absolute directory |
| `artificer remote off` | Clear the remote store |
| `artificer pull` | Add missing complete units from the remote store |
| `artificer mods` | List compile modes |
| `artificer mods on NAME`, `artificer mods off NAME` | Change one compile mode |
| `artificer --help`, `-h` | Print the command list |
| `artificer help COMMAND`, `artificer COMMAND --help` | Print the usage and options of one command |
| `artificer --version`, `-V` | Print the version |

The [reference](guide/reference.md) lists the supported build options and
the compile modes.

## Common tasks

Build through the store, then confirm that the command did not fall back:

```sh
. "$HOME/.artificer/env"
cargo check -p my-package --locked
artificer stat --json
```

In the object, `last_build` starts with `fallback:` when the last handled
command ran real Cargo.

Find out why a command falls back. The direct binary prints the reason on
stderr and exits 2:

```sh
artificer check -p my-package --locked
artificer why-fallback --limit 10
```

## Commands by task

| Job | Command |
| --- | --- |
| Check that the shim is first on PATH | `artificer doctor` |
| Read hits, misses, and the fallback count | `artificer stat --json` |
| See why one crate missed | `artificer why-miss CRATE` |
| See why commands ran real Cargo | `artificer why-fallback` |
| See where a build spent its time | `artificer profile` |
| Populate the cache | `artificer warm` |
| Free disk space | `artificer clean` |
| Carry the store between CI runs | `artificer export DIR`, `artificer import DIR` |
| Turn a compile mode on or off | `artificer mods on NAME`, `artificer mods off NAME` |
| Send every shim command to real Cargo | `artificer disable`, or `ARTIFICER_DISABLED` set to any value for a temporary bypass |
| Move the store | `ARTIFICER_HOME=PATH` |
| Cap compile workers | `ARTIFICER_JOBS=N` |

## Exit codes

| Code | Entry | Meaning |
| --- | --- | --- |
| 0 | both | The command succeeded. |
| 1 | `artificer doctor`, `artificer why-miss` | At least one `doctor` row reads `BAD`, or `why-miss` has no key record for the crate. |
| 2 | `artificer` | Artificer does not support the invocation, the store is not writable, the command is unknown, or command arguments are invalid. |
| 101 | both | An error stopped the command, including a compile failure. This is the code Cargo uses for a failed build. |
| other | both | `run` and `test` return the exit code of the program or test harness. |

Fallback is a dispatch decision, separate from a child process's exit code.
The shim runs Cargo only when Artificer declines an invocation. A program
that exits 2 returns 2 without running again. `cargo clippy` and
`cargo nextest` return their external command's exit code. They run
with the shim as `CARGO` only while caching is enabled; otherwise Cargo
runs them directly.

## Build profiles

Every `check`, `build`, `test`, `run`, and `warm` command records a build
profile, including one that falls back to Cargo. A profile holds the build's
units, their dependencies, start and end times, cache outcomes, and process
CPU time and peak memory. Artificer keeps the newest 100 profiles, and at
most 256 MiB of them. The profile files are private; the supported
interface is `artificer profile` and its `--json` output.

`cargo build --timings` and `cargo test --timings` run through Artificer.
Artificer writes `target/cargo-timings/cargo-timing-<UTC stamp>.html` and
`target/cargo-timings/cargo-timing.html`, the paths Cargo uses, and prints
`Timing report saved to PATH`.

The [profiling guide](guide/profiling.md) explains how to read a profile.

## Machine output

`artificer stat --json` prints one JSON object on one line.

| Field | Type | Meaning |
| --- | --- | --- |
| `home` | string | Store directory |
| `enabled` | boolean | Whether caching is enabled after applying `ARTIFICER_DISABLED` |
| `units` | integer | Published units in the store |
| `bytes` | integer | Size of the files in the unit directory, in bytes |
| `hits` | integer | Unit hits recorded by builds |
| `misses` | integer | Unit misses recorded by builds |
| `hit_rate` | number | `hits / (hits + misses)`; `0.0` when both are zero |
| `fallbacks` | integer | Whole-command fallbacks recorded by the shim |
| `fallback_last` | string or null | The last fallback reason |
| `builds` | integer | Handled commands recorded in `builds.jsonl` |
| `last_build` | string or null | `OP H hit, M rustc, T ms`, or `fallback: REASON` |
| `meta` | integer | Cached Cargo metadata entries |
| `scratch` | integer | Directories under `scratch/` |

Fallback reasons are free text that names the unsupported input. They are
not an enumerated set; do not match on them.

With `--message-format=json` or `--message-format=json-render-diagnostics`,
`check`, `build`, and `test` print Cargo's JSON message stream on stdout,
ending with a `build-finished` message.

### `artificer profile --json`

Prints one JSON object on one line. Times are whole milliseconds. Unit
times are relative to the start of the build.

| Field | Type | Meaning |
| --- | --- | --- |
| `id` | string | Profile ID |
| `command` | array of strings | The command line that ran |
| `started_at_ms` | integer | Build start, Unix time in milliseconds |
| `wall_ms` | integer | Wall-clock time of the whole command |
| `cpu_ms` | integer | User plus system CPU time of every measured process |
| `cores` | integer | Host core count |
| `jobs` | integer | Compile worker cap |
| `result` | string | `built`, `failed`, or `fallback` |
| `fallback` | string or null | Fallback reason |
| `units` | object | `total`, `hit`, `miss`, and `failed` unit counts |
| `setup_ms` | integer | Time before the first unit started |
| `tail_ms` | integer | Time after the last unit ended |
| `parallelism` | object | See below |
| `critical_ms` | integer | Length of the critical path: duration plus wait of each step |
| `critical_path` | array of units | The dependency chain that ended last, earliest first |
| `top_units` | array of units | The 10 longest units |
| `phases` | array | Per-phase totals, longest first |
| `low_parallelism` | array | Up to 5 windows of at least 1 s when at most a quarter of the jobs ran |
| `compiler` | array | Compiler pass totals by category; empty unless `ARTIFICER_PASSES` was set |

`parallelism` fields: `average` (number, mean processes running between the
first and last compile process), `peak` (integer, most processes running at
once), `busy_ms` (integer, process time summed over all processes), and
`idle_core_ms` (integer, job slots left unused in that interval).

A unit object has `unit` (integer index), `name`, `version`, `role`
(`package` or `targets`), `outcome` (`hit`, `miss`, `failed`, or null),
`start_ms`, `end_ms`, `duration_ms`, `wait_ms` (time between the unit's
dependencies finishing and the unit starting), `cpu_ms`, and
`peak_rss_bytes` (integer, bytes).

A phase object has `phase` (phase name, such as `rustc`, `key`, or
`metadata`), `stage` (`setup`, `wrapper`, `unit`, `process`, or `run`),
`count`, `total_ms`, and `cpu_ms`.

A window object has `start_ms`, `end_ms`, `running` (most processes running
in the window), and `units` (names of units that ran in it).

A compiler object has `category` (`parse`, `expand`, `resolve`, `checks`,
`typeck`, `borrowck`, `mono`, `metadata`, `codegen`, `llvm`, `incremental`,
`link`, or `other`) and `total_ms`.

### `artificer profile list --json`

Prints one JSON array, newest first. Each object has `id`, `started_at_ms`,
`wall_ms`, `result`, `hit`, `miss`, `failed`, and `command`, with the same
meanings as above. A profile that cannot be read is left out.

### `artificer profile diff --json`

Prints one JSON object. `base` and `head` are the two IDs. `wall_ms`,
`cpu_ms`, `hit`, and `miss` are objects with `base`, `head`, and `delta`
(head minus base). `phases` lists `phase`, `base_ms`, `head_ms`, and
`delta_ms`, largest change first. `units` lists up to 15 units with the
largest change in duration. Units match by package and role. Each has
`package`, `name`, `version`, `role`, `base_ms` and `head_ms` (null when
the unit is on one side only), `delta_ms`, `base_outcome`, and
`head_outcome`. `base_critical_path` and `head_critical_path` list the
unit names on each critical path.

## Conventions

- The shim records fallback reasons without printing them. Inspect
  `artificer stat`, `artificer why-fallback`, and `artificer doctor`.
- If the store is not writable, the shim runs Cargo; the direct binary exits 2.
- `ARTIFICER_HOME` sets the store directory. The default is the OS cache
  directory.
- `ARTIFICER_JOBS` caps compile workers. If it is unset, Artificer reads
  `CARGO_BUILD_JOBS`. If both are unset, it uses the host core count.
- `ARTIFICER_STORE_CAP_GB` sets the store cap in GiB. It must be an
  integer; any other value is an error. The default is 15% of the volume,
  with a minimum of 8 GiB. Larger configured values are clamped to that limit.
- `ARTIFICER_REMOTE` overrides the configured remote store. An empty value
  turns the remote off.
- `ARTIFICER_NO_TREE` skips the feature probe, so every handled command
  runs real Cargo.
- `ARTIFICER_PASSES`, set to any value, records rustc's pass timings in
  the build profile. Artificer runs that rustc with `RUSTC_BOOTSTRAP=1` and
  `-Ztime-passes-format=json`. The timings do not print to the terminal,
  and unit keys do not change.
- `ARTIFICER_TIMING` prints Artificer phase timings.
- Compile modes live in `ARTIFICER_HOME/mods.toml`. Change them with
  `artificer mods`, not by editing the file.

The [reference](guide/reference.md#environment-variables) lists every
environment variable.
