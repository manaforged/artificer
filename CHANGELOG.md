# Changelog

The minimum supported Rust version is 1.98.

Artificer is in the `0.x` series. Within a minor series such as `0.1.x`,
updates keep the command-line interface, exit codes, and `stat --json`
output compatible. A breaking change to those, a new cache format, or a
higher minimum Rust version needs a new minor release, such as `0.2.0`.

## Unreleased

- A test binary or doctest run that fails without printing a test failure, such as one killed by a signal, is now named with its exit status ("process didn't exit successfully: `<path>` (signal: 6 (SIGABRT))"), as Cargo does. Before, the run exited 1 with no message.
- A workspace crate built with `CARGO_INCREMENTAL=0` and later built with incremental compilation on (or the other way round) no longer fails with "no rmeta/rlib for <crate>". Both placements now name a unit's files with the same stem, and a cached unit whose files do not match is rebuilt instead of restored.
- The `threads` mode is on by default. It passes rustc `-Z threads` only
  when rustc accepts the flag, so builds with a stable toolchain do not
  change. A probe made with `RUSTC_BOOTSTRAP` set no longer applies to
  builds without it, where rustc would reject the flag.
- `artificer clean` deletes the compile directories under
  `target/<profile>/.artificer/`, except one a running build is compiling
  in. A pipelined dependent reads its dependency's metadata from the
  cache, so a clean or a second build cannot remove it mid-build.
- A unit is keyed on its own target's files. Editing an integration
  test, example, or bench no longer rebuilds the library, the binaries,
  or the other tests, as with Cargo. Before, any file in the package
  changed every unit's key, which also caught a proc macro reading an
  untracked file under `tests`. Files another target's compile reads
  are now tracked through rustc's dependency records only, as Cargo does.
- A build script that emits `rerun-if-changed` or `rerun-if-env-changed`
  runs again only when those inputs change, as with Cargo. A script that
  emits neither still runs again when any file in its package changes.
  Crates with build scripts recompile once after the upgrade.
- A package's tests, checked targets, and extra binaries compile at the
  same time, as with Cargo. They had compiled one at a time, so a library
  edit followed by `cargo test` rebuilt each test binary in turn.
- Tests, examples, and binaries pass rustc a `-C metadata` value, as
  with Cargo, so same-named tests in two packages keep separate
  incremental state.
- Rebuilding an edited workspace crate reuses rustc's incremental cache,
  as Cargo does. Each workspace crate compiles in a stable directory
  under `target/<profile>/.artificer/`, and its outputs are then published
  to the cache. Each edit had compiled in a new directory, which made
  rustc recompile the whole crate. The first build after upgrading
  recompiles workspace crates once.
- An unchanged source file costs one file-system check per build. The
  digest cache records which environment variables a file reads, and a
  crates.io dependency's build script is keyed on the package version
  instead of a scan of its files.
- rustc runs straight from the toolchain's sysroot, as with Cargo,
  instead of through the rustup proxy on every compile. With `RUSTC` set,
  that program runs as given.
- A proc-macro crate compiles with Cargo's `build-override` defaults, as
  Cargo does: no debug info and no separate metadata file. It had used
  the build's profile, which put full debug info into every derive crate.
- A new `slim-deps` mode, on by default, compiles crates.io and git
  dependencies with line-table debug info. Workspace and path crates keep
  the profile's debug info, so they debug as before, and backtraces keep
  file and line for every crate. It replaces the `slim` mode, which
  lowered debug info for every crate; set `debug = "line-tables-only"` in
  a Cargo profile for that.
- The `cranelift` and `linker` modes are removed, with the
  `ARTIFICER_CODEGEN` and `ARTIFICER_LINKER` variables. A Cargo-configured
  linker still applies. A `mods.toml` that names `cranelift`, `linker`, or
  `slim` still loads.
- A library is keyed on the build-script run it compiles against. Two
  builds whose script runs differ, for example in `RUSTDOC`, no longer
  rebuild one library unit in turn, which sent a build to Cargo with
  `unit ... is in use` while the other build held the unit. Libraries of
  packages with build scripts compile once after updating.
- `CARGO_PROFILE_<NAME>_<KEY>` environment variables for the profile keys
  Artificer applies, such as `CARGO_PROFILE_DEV_DEBUG=0`, go through the
  cache instead of running Cargo. They override the manifest's profile, as
  in Cargo. A value Cargo would reject, `INHERITS`, and the `PACKAGE` and
  `BUILD_OVERRIDE` forms still run Cargo.
- A new `early` mode, off by default, starts a crate's dependents in
  `check` builds as soon as rustc writes its early metadata: the item
  interfaces, before function bodies are checked. It needs a rustc that
  accepts `-Z early-metadata`; with stock rustc nothing changes. Other
  builds pass the flag too, and build profiles record when each crate
  wrote its early metadata and its full metadata.
- `check` builds start a crate's dependents once its metadata is written,
  as `build` does, rather than after every crate below it finishes. A
  crate that a proc macro or build script also needs no longer holds up
  the crates that only check against it.
- `artificer push` sends the remote store the units it lacks. A build with
  a remote store set pushes the units it compiled in the background, so
  every machine that shares the store fills it; over SSH the remote host
  adds them with `artificer import`.
- rustc gets a job token pool that its build owns, as with Cargo, so its
  extra codegen and frontend threads run only on free cores; the pool is
  removed when the build ends.
- A new `trust` mode, on by default, passes `-Z trusted-crate` to crates.io
  dependencies when rustc accepts it. That rustc skips checks that only
  report errors, such as borrowck and lints, for crates that already
  compile. Stock rustc does not accept the flag, so nothing changes with
  it.
- With a remote store set, a build takes each unit it misses from the
  remote store before compiling it: a copy from a directory, or one rsync
  over a shared SSH connection. A cold build no longer waits for the
  background pull. Profiles show the time as `fetch`.
- A build killed while it held job tokens no longer stalls every other
  build on the machine. Each held token is recorded under a lock the
  system releases when its process dies, and a build that waits two
  seconds for a token takes back any whose holder is gone.
- The `threads` mode uses up to 8 frontend threads instead of a quarter
  of the cores.
- The `threads` mode takes effect again. Its compiler probe wrote into a
  directory it never created, so the probe failed and the mode stayed
  off.
- A build no longer hangs when its standard error closes early, as in
  `cargo build 2>&1 | head`. Artificer's own messages ignore a closed pipe,
  and a build worker that crashes now fails the build instead of leaving
  the others waiting forever.
- A package used only at build time, by a build script or a proc macro,
  compiles with Cargo's `build-override` defaults: no debug info, no
  optimization, no embedded bitcode, and unwinding panics. Its build script
  sees the matching `OPT_LEVEL` and `DEBUG`. Artificer had compiled it with
  the full profile, which spent more time on `syn` and similar crates, and
  on `opt-level = 3` in release builds. Build scripts compile without
  embedded bitcode, as with Cargo.
- In a workspace with `default-members`, a command without `-p` or
  `--workspace` builds only the default members, as Cargo does. Artificer
  built every member.
- A build script runs as its own unit, as with Cargo: it starts once its
  build-dependencies are built, while the package's other dependencies
  still compile. Profiles show it as a `(build script)` unit.
- A library starts compiling once its dependencies have written their
  metadata, as with Cargo's pipelining; binaries, tests, and proc macros
  still wait for every dependency to finish. Build profiles record each
  unit's metadata time.
- Artificer raises its open-file limit at startup, as Cargo does. A cold
  build of a large workspace on macOS stopped with "Too many open files".
- A test target with `harness = false` runs its own `main` again. Artificer
  had compiled it with the libtest harness, so it reported `running 0
  tests` and passed. `cargo check --tests` leaves out its `#[test]`
  functions, as Cargo does.
- Every `check`, `build`, `test`, `run`, and `warm` command records a build
  profile. `artificer profile` shows
  where the newest build spent its time: the critical path, idle cores,
  Artificer setup time, phases, and the longest units. `profile list`
  lists recorded builds, `profile diff` compares two, `--json` prints
  machine output, `--trace` writes a Chrome trace for Perfetto, and
  `--html` writes an HTML report.
- `cargo build --timings` and `cargo test --timings` run through
  Artificer and write Cargo's timing report, instead of falling back to
  Cargo.
- `ARTIFICER_PASSES` records rustc pass timings in the build profile
  instead of printing them.
- `-p` with a glob pattern or a package ID URL now runs Cargo instead of
  failing.
- When Artificer cannot use its store directory, the fallback reason says to
  remove it or set `ARTIFICER_HOME` to an empty directory.
- On Windows, a program or test whose exit code is above 255 no longer
  reports success.
- Build output uses Cargo's status lines: `Compiling`, `Checking`,
  `Finished`, `Running`, and `Executable`, in color when the terminal or
  `CARGO_TERM_COLOR` allows it. `--color auto|always|never` now takes
  effect and overrides `CARGO_TERM_COLOR`, as it does in Cargo.
- `cargo test` and `cargo run` print the `Finished` line before `Running`.
- Compiler messages show file paths relative to the workspace root, as Cargo
  does.
- A failed build prints one `could not compile` line per crate as that crate
  fails, then `build failed, waiting for other jobs to finish...` while other
  crates still compile. Build script failures name the build script, and a
  build script that exits with an error prints Cargo's
  `failed to run custom build command` message with its output.
- Test binaries run from `target/<profile>/deps`, as with Cargo.
- `cargo run --bin` and `cargo run --example` run the program from the
  target directory, so `std::env::current_exe()` points there.
- `artificer help COMMAND` and `artificer COMMAND --help` print the usage of
  one command.
- `artificer why-miss` exits 1 when it has no key record for the crate.
- Warm builds reuse cached build-script results faster.
- On Windows, install and uninstall recognize a user PATH entry written with
  a variable such as `%USERPROFILE%`, so install does not add a duplicate,
  and they refuse to change a user PATH that is not valid UTF-16.
- `artificer doctor` checks `rust-analyzer.toml`, the file name
  rust-analyzer reads, instead of `config.toml`.
- `artificer uninstall` deletes shell profile files that install created
  when they are empty after the PATH line is removed, and removes
  `~/.artificer` when nothing else is in it.
- `artificer install` that cannot find the real Cargo leaves no files or
  directories behind.
- The store keeps one unit per workspace crate, target kind, and settings
  for each checkout. A rebuild after an edit evicts the unit it replaces,
  including the units of crates that depend on the edited crate. Before,
  every edit added a unit that stayed until the age limit or the size cap.
- `artificer clean` and garbage collection remove scratch directories that
  have no published unit and are more than an hour old.
- A workspace that has two packages with the same name and version from
  different sources, such as a path crate and a git dependency, now builds
  through the cache instead of running Cargo.
- `--target-dir DIR` on `build`, `check`, `test`, `run`, and `clean` now
  goes through the cache instead of running Cargo. As in Cargo, it wins
  over `CARGO_TARGET_DIR` and `build.target-dir`, and a relative path is
  relative to the current directory.
- `[resolver] incompatible-rust-versions` in Cargo configuration no longer
  sends builds to Cargo. It only changes how versions are picked when the
  lockfile is written. Other `[resolver]` keys still do.
- A store written by an earlier release, whose `CACHEDIR.TAG` holds only the
  signature line, is adopted and its tag updated. Before, the store was
  refused and every build ran Cargo.
- Path packages compile incrementally where Cargo would, in
  `target/<profile>/incremental`. `CARGO_INCREMENTAL`, `CARGO_BUILD_INCREMENTAL`,
  and `profile.<name>.incremental` apply as in Cargo. The `sweep` mode no
  longer deletes incremental directories during builds.
- File content digests are kept in the store and reused while a file's
  length, modification time, inode, and change time stay the same.
- A package that build scripts or proc-macros use with different features
  than normal code is now built once per side, as Cargo does, instead of
  sending the whole build to Cargo.
- Final links get only the library search paths of the crates they link.
  A dependency's `rustc-link-lib` and `rustc-link-arg` no longer reach
  dependents, and build-dependencies no longer add anything to the link.
- Units no longer record the store's path. Build-script output, recorded
  inputs, and compiled output name the store with a placeholder, so units
  exported from one store hit after import into a store at another path.
- `artificer remote` and `artificer pull` copy complete units from another
  machine's store over SSH or from a directory. Handled builds start a
  background pull every 15 minutes when a remote is set.
  `artificer install --remote LOCATION` sets it up.
- Build-script units no longer depend on the machine's CPU count, the
  Cargo home path, or paths inside compiled objects in `OUT_DIR`, so they
  hit on other machines and in moved checkouts.
- `check` and `build` accept `--lib`, `--bin`, `--bins`, `--example`,
  `--examples`, `--test`, and `--tests`, and `check` also `--bench` and
  `--all-targets`, through the cache. `test` accepts `--no-fail-fast`.
- `[env]` and `[patch]` in Cargo configuration, and a `linker` or `runner`
  for the host target, now build through the cache. `[env]` values reach
  rustc, build scripts, rustdoc, tests, and `run`, and are part of the key.
- A build script is compiled once per change to `build.rs`, its
  build-dependencies, features, or compiler. Editing other package files
  reruns the compiled script, as Cargo does, instead of compiling it again.

## 0.1.1 - 2026-09-26

- With `CARGO_TERM_COLOR=always`, as many CI systems set it, Artificer
  misread the colored output of `cargo tree` and sent every command in a
  project with a shared dependency to Cargo. Artificer now asks
  `cargo tree` for plain output.
- On Windows, `-p` with a package ID that spells its path with 8.3 short
  names now matches the package.

## 0.1.0 - 2026-09-26

First public release. Yanked: with `CARGO_TERM_COLOR=always`, every command
fell back to Cargo. Fixed in 0.1.1.

- Cargo shim for `check`, `build`, `test`, and `run`. Unsupported invocations
  use Cargo with their original arguments.
- Compile units shared across checkouts by source content, compiler settings,
  features, dependency artifacts, and build-script output.
- Cache validation against rustc dependency records, including included files
  outside the package and the distinction between unset and empty environment
  values.
- Build scripts receive resolved profile settings and encoded Rust flags.
- Unix builds share a jobserver without replenishing tokens held by active builds.
  Windows schedules workers within each process.
- Optional local daemon with environment checks before reuse.
- Cache export and import. Imported files are copied so later changes to the
  exported directory cannot alter the live cache.
- Install and uninstall commands, cache maintenance, diagnostics, and
  `artificer stat --json` for tooling.
- `cargo install`, `cargo uninstall`, and `cargo clean` retain Cargo's behavior.
- `artificer install` adds the shim to PATH in `~/.profile`, in
  `$ZDOTDIR/.zshenv` (or `~/.zshenv` when `ZDOTDIR` is unset) when `SHELL` is
  zsh or that file exists, and in `~/.bashrc` and `~/.bash_profile` when
  present, so non-interactive
  shells started by editors and scripts use it.
  On Windows it puts the shim first on the user PATH. `--no-modify-path`
  skips this.
- `artificer uninstall` removes those PATH lines and, when Cargo installed
  Artificer, runs `cargo uninstall artificer-build`. `--purge` also deletes
  the cache.
- The shim finds `rustc` next to the real Cargo when `rustc` is not on PATH.
- `artificer enable` and `artificer disable` persist caching state.
  `ARTIFICER_DISABLED`, set to any value, bypasses the shim for one command.
- Published on crates.io as `artificer-build`, which installs the `artificer`
  binary.

