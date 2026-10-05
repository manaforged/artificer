# Artificer

Artificer is a compile cache for Rust that every checkout on your machine
shares. It puts a `cargo` shim ahead of the real Cargo on your PATH, so
`cargo build` and `cargo test` work as before. If any checkout already
compiled a crate from the same inputs, the shim reuses that output instead
of running rustc.

Use it when several checkouts build the same dependencies: git worktrees
or CI runners that each start with an empty `target/`.

## Requirements

- Rust 1.98 or later, installed with rustup
- macOS (arm64), Linux (x86_64), or Windows (x86_64)

## Install

1. Install the binary. The crates.io package is `artificer-build`, and
   the command it installs is `artificer`:

   ```sh
   cargo install artificer-build --locked
   ```

2. Install the shim:

   ```sh
   artificer install
   ```

   This puts the shim in `~/.artificer/bin`. It adds this line to
   `~/.profile`, to `.zshenv` in `$ZDOTDIR` or your home directory if you
   use zsh, and to `~/.bashrc` and `~/.bash_profile` if they exist:

   ```sh
   [ ! -f "$HOME/.artificer/env" ] || . "$HOME/.artificer/env"
   ```

   On Windows, it puts the shim first on your user PATH. To edit your
   profiles yourself, run `artificer install --no-modify-path`.

3. Open a new shell and check the setup:

   ```sh
   artificer doctor
   ```

   A working setup has no `BAD` rows.

[Install](docs/guide/install.md) covers editors, CI, and Windows machines
with Rust installed for all users.

## Usage

```sh
cargo build                    # compiles and stores each crate
git worktree add ../second
cd ../second && cargo build    # reuses the stored crates
artificer stat                 # hits, misses, and cache size
```

If a crate in your workspace compiles again when you did not expect it,
run `artificer why-miss CRATE`. If a command went to real Cargo, run
`artificer why-fallback`.

To skip Artificer for one command, run `ARTIFICER_DISABLED=1 cargo build`.
To turn it off until you run `artificer enable`, run `artificer disable`.

## How it works

- Artificer identifies each compiled crate by its source, compiler, flags,
  features, dependencies, and build-script output. The checkout path is
  not part of that identity, so worktrees share entries. An input that
  contains the path, such as `env!("CARGO_MANIFEST_DIR")`, an absolute path
  in `RUSTFLAGS`, or build-script output that prints the path, stops that
  crate and the crates that depend on it from sharing.
- On a hit, Artificer rechecks the files and environment variables that
  rustc recorded. On a miss, it runs rustc once. A failed or killed
  compile never becomes a reusable entry.
- If Artificer does not support part of a command, real Cargo runs the
  whole command with the same arguments.

The [cache model](docs/guide/cache-model.md) and the
[safety contract](docs/guide/safety-contract.md) have the details.

## Cache

The cache is per user. It is in `~/Library/Caches/artificer` on macOS,
`$XDG_CACHE_HOME/artificer` or `~/.cache/artificer` on Linux, and
`%LOCALAPPDATA%\artificer` on Windows. Set `ARTIFICER_HOME` to use a
different directory.

Builds evict the entries unused the longest, tracked to the day, to keep
the cache near its cap: 15% of the disk, or 8 GiB if that is larger.
Entries unused for 30 days are removed. Set `ARTIFICER_STORE_CAP_GB` to lower the cap.

Artificer does not connect to other machines. It runs no background
service unless you turn on the optional daemon, which listens on
`127.0.0.1` only.

## Uninstall

```sh
artificer uninstall          # remove the shim and the PATH line; keep the cache
artificer uninstall --purge  # also delete the cache
```

If Cargo installed Artificer, `artificer uninstall` also runs
`cargo uninstall artificer-build`. On Windows, it prints that command.

## Limits

- `--target` builds, `cargo bench`, and custom profiles go to real Cargo.
- `cargo test` runs test binaries and doctests at the same time, up to the
  job limit. Cargo runs them one at a time. If tests in different binaries
  share a port, a file, or a database, set `ARTIFICER_JOBS=1`, which also
  limits compile jobs.
- Artificer does not enable incremental compilation. After a small edit in
  one checkout, plain Cargo can be faster.
- The cache is local to one machine. CI can move it with
  `artificer export` and `artificer import`. Cached files are executable
  code, so import only exports you trust.
- Artificer is at version 0.x. The [changelog](CHANGELOG.md) states what
  each release keeps compatible.
- Measured with Artificer 0.1.0 at commit `a8e4a1b`, on macOS only: on
  ripgrep 14.1.1 (Apple M2 Max), a second checkout builds in 0.15 s
  instead of 5.23 s, and a cold build takes about as long as Cargo. A small
  edit takes 1.34 s instead of 0.52 s.
  [Measurements](docs/guide/measurements.md) has the method, the raw
  samples, and how to measure your own project.

## Docs

- [User guide](docs/guide/README.md)
- [Reference](docs/guide/reference.md): commands, options, and environment
  variables
- [Command-line API](docs/api.md): exit codes and `stat --json`

## Contributing

Artificer does not accept external pull requests until its API is more
stable. Report bugs and request features in
[GitHub issues](https://github.com/manaforged/artificer/issues).
[CONTRIBUTING.md](CONTRIBUTING.md) describes the maintainers' workflow, and
[SECURITY.md](SECURITY.md) explains how to report a vulnerability.

## API

<!-- truesight:surface -->

224 public items (23 types, 58 functions, 19 methods, 95 fields, 26 variants, 3 constants) at 224 paths; 0 are re-export aliases. 99 trait impl lines, 6 inherent impl lines. 1 module, 0 re-export only.

| Module | Items | Types | Functions | Methods | Aliases |
| --- | ---: | ---: | ---: | ---: | ---: |
| `artificer` | 224 | 23 | 58 | 19 | 0 |

<!-- /truesight -->

## License

Artificer is licensed under the [MIT license](LICENSE).

Copyright 2026 Manaforge Technologies, LLC.
