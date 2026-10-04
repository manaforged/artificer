# Safety contract

If Artificer does not support any part of a command, real Cargo runs the
whole command with the original arguments. The command falls back when it
sees:

- a flag, command, or target selection it does not support (`--target`,
  benches, custom profiles, aliases, `--keep-going`, and more);
- an unknown key in an active `[profile]` chain, or a lint level it does
  not apply;
- a `[profile]` table, `build.rustc`, `build.target`, or target
  linker/runner in `.cargo/config.toml`;
- Cargo-owned environment it would otherwise ignore (`CARGO_PROFILE_*`
  other than the profile keys it applies, `CARGO_UNSTABLE_*`,
  `CARGO_TARGET_*` except `CARGO_TARGET_DIR`, and the `CARGO_BUILD_*`
  settings that change the build);
- a feature probe (the step that asks Cargo which features are enabled)
  that cannot resolve the per-invocation feature set;
- a package that Cargo builds with different features for build scripts
  and for normal code. Artificer compiles each package once per build, so
  it hands these builds to Cargo.

The [reference](reference.md#supported-options) lists the supported options.

Set `ARTIFICER_NO_TREE=1` to skip the feature probe on purpose. Every
command then runs stock Cargo.

A fallback is a normal stock Cargo build into the workspace `target/`.
The shim prints nothing by design, but it records the reason:
`artificer stat` prints the fallback count and the last reason,
`artificer why-fallback` lists the reasons by count, and
`artificer doctor` shows a `fallbacks` row.

Run the `artificer` binary directly to see the fallback reason on
stderr. The direct binary then exits 2.
