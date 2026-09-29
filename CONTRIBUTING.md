# Contributing to Artificer

Artificer compiles Rust packages outside Cargo's ordinary build directory. A
change can appear correct while producing a different artifact or command
result. Keep each change narrow and add a comparison with Cargo when behavior
changes.

## Set up the repository

Install Rust 1.98 or later through Rustup. Add Clippy, then install
cargo-nextest and cargo-deny to run the same local checks used before submitting a change.

~~~sh
rustup component add clippy
cargo install cargo-nextest --locked
cargo install cargo-deny --locked
~~~

Build and test with the real Cargo executable, not an installed Artificer shim.
If the shim is active, use the rustup Cargo proxy directly or remove
$HOME/.artificer/bin from PATH for this shell.

To build this repository itself, use rustup Cargo, not the shim:

~~~sh
CARGO="$HOME/.cargo/bin/cargo" $HOME/.cargo/bin/cargo build
~~~

## Make a change

1. Open an issue before a large compatibility or store-format change.
2. Keep production test bodies in sibling `*_tests.rs` files.
   Put cross-module behavior tests under `tests/`.
3. Add a parity test when a change affects Cargo-visible behavior.
4. Do not add performance numbers without an output-equivalence check and a
   reproducible benchmark record.
5. Record user-visible behavior in CHANGELOG.md under Unreleased.

Run these checks:

~~~sh
cargo fmt --all -- --check
cargo clippy --all-targets --locked -- -D warnings
cargo check --all-targets --locked
cargo nextest run --locked
cargo test --doc --locked
cargo deny check
~~~

On macOS or Linux, also validate the installer syntax:

~~~sh
sh -n scripts/install.sh
~~~

On Windows, run:

~~~powershell
pwsh -NoProfile -File scripts/install.ps1 -Help
~~~

Run these checks with rustup's Cargo, not an installed Artificer shim. The
shim is the product under test.

## Invariants

- The supported interface is the `artificer` CLI, the `cargo` shim, exit
  codes, and `artificer stat --json` ([docs/api.md](docs/api.md)). The Rust
  library is not an embedding API.
- Store files are private. Units move between stores only through
  `artificer export` and `artificer import`.
- An Artificer failure is a bug in this repository. Report the exact error
  and fix the cause; do not work around it with real Cargo.
- On `can't find crate` or `found possibly newer version`, suspect a stale
  unit in the store before the code.

## Submit a pull request

Explain the incorrect or missing behavior, the chosen boundary, and the
verification you ran. Keep formatting-only changes separate from behavior
changes when possible.

By submitting a contribution, you agree to license it under the MIT License.

## Release a version

Artificer ships through crates.io only. The project does not create Git
tags, GitHub releases, or prebuilt binaries. The published crate records
its source commit in `.cargo_vcs_info.json`.

The toolchain in `rust-toolchain.toml` and `rust-version` in `Cargo.toml`
name the same release, so CI tests the minimum supported Rust version.
Change them together.

Pull requests with code changes run Linux checks. Each code change that
lands on `main` runs Linux, macOS, and Windows. Documentation-only changes
run the content check. To release:

1. Merge the version change and its finalized changelog entry through a
   pull request. Record a benchmark from `scripts/bench.py` in the entry
   only with its full machine and toolchain header.
2. Wait until CI passes on that `main` commit on all three platforms.
3. Run the Cargo beta workflow through `workflow_dispatch`. It checks
   compatibility with upcoming Cargo versions.
4. From a clean checkout of that commit, run
   `cargo publish --dry-run --locked` and check the file list.
5. Run `cargo publish --locked` from the same checkout.

The crates.io package is `artificer-build`; the crates.io name `artificer`
belongs to an unrelated crate. The package installs the `artificer` binary,
and `artificer install` configures the shim from it.
