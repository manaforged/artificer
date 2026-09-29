# Changelog

The minimum supported Rust version is 1.98.

Artificer is in the `0.x` series. Within a minor series such as `0.1.x`,
updates keep the command-line interface, exit codes, and `stat --json`
output compatible. A breaking change to those, a new cache format, or a
higher minimum Rust version needs a new minor release, such as `0.2.0`.

## Unreleased

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

