# Measurements

Run the benchmark on the machine and toolchain you intend to use, from a
clone of this repository. It is tested on macOS. It builds the release
binary first, and all work goes under `target/bench-work`.

## Public fixture

```sh
python3 scripts/bench.py --public
```

The fixture is ripgrep 14.1.1. The first run clones it from GitHub and
fetches its dependencies, so it needs network access. The timed builds run
with `--offline`. Each sample starts with an empty Artificer store and
measures four cases:

| Case | What is timed |
| --- | --- |
| cold | The first build of a fresh checkout, empty store |
| one-edit | A rebuild after changing the text `rg --version` prints |
| clean | A rebuild after deleting `target/`, with the store from the builds above |
| second-checkout | A build of a second copy of the tree, with its own empty `target/` and the shared store |

For stock Cargo, clean and second-checkout rebuild everything, because Cargo
has no cache outside `target/`. That difference is what Artificer adds.

## Generated workspace

```sh
python3 scripts/bench.py
```

This builds a generated workspace (16 crates by default) and measures cold,
unchanged, second-checkout, parallel, edited, check, and test builds, plus
nextest when it is installed. The parallel case starts four builds of the
same tree at once, each with Cargo's default job count. It runs after the
cold and second-checkout cases, so Artificer's store is already warm and
the case measures reuse by concurrent builds, not a shared cold compile.
`--quick` is a smoke run with 8 crates and one sample, not a measurement.

## What every sample checks

- The built program prints the expected output. For one-edit, the output
  must show the edit.
- Artificer's store counters match the case: cold compiles, the warm cases
  only hit, and edits recompile.
- No command falls back to real Cargo. A fallback aborts the run, because
  its time would be Cargo's time under Artificer's name.

## Reading the results

Samples alternate which driver runs first. The table shows the median and
the min-max range in seconds. `target/bench-results.json` keeps every
sample, the machine, the Artificer commit, and the toolchain. Publish that
file with any number you quote.

Stock Cargo runs with its defaults, including incremental compilation. The
test cases include one scheduling difference: Artificer runs test binaries
at the same time, and Cargo runs them one at a time. Artificer does not use
incremental compilation, so a small edit in one checkout can be faster
with Cargo.

## Results for 0.1.0

ripgrep 14.1.1, `cargo build --locked --offline`, 3 samples on an Apple M2
Max (12 CPUs, macOS), rustc and Cargo 1.98.0, Artificer at commit
`a8e4a1b`. Median seconds, with the min-max range:

| Case | Stock Cargo | Artificer |
| --- | --- | --- |
| cold | 5.71 (5.08-5.97) | 5.59 (5.58-5.61) |
| clean | 5.31 (4.95-5.60) | 0.09 (0.08-0.09) |
| second-checkout | 5.23 (5.01-5.34) | 0.15 (0.15-0.16) |
| one-edit | 0.52 (0.51-0.63) | 1.34 (1.28-1.46) |

The raw samples are in
[`bench/2026-09-26-ripgrep-14.1.1-m2-max.json`](../../bench/2026-09-26-ripgrep-14.1.1-m2-max.json).
