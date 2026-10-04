# Profiling a build

Every `check`, `build`, `test`, `run`, and `warm` command records a build
profile. A command that falls back to Cargo records one too, so the
fallback reason and its cost are visible. Artificer keeps the newest 100
profiles.

```sh
cargo build
artificer profile
```

`artificer profile` reads the newest profile. Give an ID, or a unique
prefix of one, to read an older build. `artificer profile list` shows the
recorded builds, newest first.

## Read a profile

The report has these sections. Empty sections are left out.

- **Summary.** Wall time, total CPU time, jobs and cores, and unit counts
  by outcome. `setup` is the time before the first unit started. `tail` is
  the time after the last unit ended, such as linking or running tests.
- **Critical path.** The chain of dependent units that ended last. The
  build cannot finish before this chain does. Each step shows when it
  started, how long it ran, and its wait: the time between its last
  dependency finishing and the step starting. A long wait means the unit
  was ready but no worker was free.
- **Where the time went.** Totals per phase, such as `rustc`, `key`, and
  `publish`. Artificer's own setup phases, such as `metadata` and
  `feature-probe`, are in a separate group.
- **Top units.** The longest units, with wait, CPU time, peak memory, and
  outcome.
- **Idle cores.** Windows of at least one second when at most a quarter of
  the jobs ran. Each window lists the units that ran in it. These units
  hold up the rest of the build.
- **Compiler passes.** Time spent in each rustc pass category, such as
  type checking, borrow checking, and LLVM. This section shows only when
  the build ran with `ARTIFICER_PASSES` set.

Total CPU time far below wall time multiplied by jobs means the build was
not parallel. Look at the critical path and the idle-core windows first.

## Profile a cold build

A cold build compiles every unit. Use an empty store so that no unit hits:

```sh
store="$(mktemp -d)"
ARTIFICER_HOME="$store" cargo build
ARTIFICER_HOME="$store" artificer profile
```

Profiles live in the store, so read the profile with the same
`ARTIFICER_HOME`.

## Compiler passes

```sh
ARTIFICER_PASSES=1 cargo build
artificer profile
```

Artificer runs rustc with `RUSTC_BOOTSTRAP=1` and
`-Ztime-passes-format=json`, and records the timings in the profile. Unit
keys do not change.

## Timeline views

`--trace FILE` writes a Chrome trace. Open it in
[Perfetto](https://ui.perfetto.dev) to see each worker as a track.

`--html FILE` writes a self-contained HTML report.

`cargo build --timings` and `cargo test --timings` write Cargo's timing
report to `target/cargo-timings/`.

## Compare two builds

Record a build before and after a change, then compare them:

```sh
artificer profile list --limit 2
artificer profile diff BASE HEAD
```

The diff shows the change in wall time, CPU time, hits, and misses; the
change per phase; the units whose duration changed most, with outcome
changes such as `hit -> miss`; and the critical path of each build.

All three commands accept `--json`. The [API reference](../api.md#build-profiles)
documents the fields.
