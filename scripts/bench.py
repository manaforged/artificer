#!/usr/bin/env python3
"""Artificer benchmark harness.

Measures stock Cargo against Artificer on one generated workspace:

  cold             first build in a fresh checkout and store
  noop             the same build again, nothing changed
  second-checkout  the same tree in a second directory, shared store
  parallel         four identical builds at once
  edit             one mid-graph crate changed, rebuilt
  check            cargo check after the builds
  test             cargo test --workspace (one 200ms test per crate)
  test-warm        the same cargo test again
  nextest          cargo-nextest run, if installed
  nextest-warm     the same nextest run again

Every build measurement runs the produced binary and asserts it prints
the expected value. Test cases must exit 0. A fast number from a wrong
build is not a measurement.

Work lives under target/bench-work, not /tmp.

Samples alternate which driver runs first, so neither side always gets
the cold page cache. The table shows the median and the min-max range;
target/bench-results.json keeps every sample. Timings hold for this
machine only. `--quick` is a smoke run with one sample, not a
measurement.

Usage:
  python3 scripts/bench.py [--quick] [--crates N] [--samples N]
                           [--artificer PATH] [--stock PATH] [--keep]
  python3 scripts/bench.py --public   # ripgrep 14.1.1, README cases
"""


import argparse
import json
import os
import shutil
import statistics
from bench_generated import measure
from bench_public import RIPGREP_TAG, measure_public
from bench_support import EXE, find_stock, machine_header, run
from pathlib import Path

GENERATED_CASES = [
    "cold",
    "noop",
    "second-checkout",
    "parallel",
    "check",
    "test",
    "test-warm",
    "nextest",
    "nextest-warm",
    "edit",
]
PUBLIC_CASES = ["cold", "clean", "second-checkout", "one-edit"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--quick", action="store_true", help="smoke run: 8 crates, 1 sample")
    parser.add_argument("--crates", type=int, default=16)
    parser.add_argument("--samples", type=int, default=3)
    parser.add_argument("--artificer", default=None)
    parser.add_argument("--stock", default=None)
    parser.add_argument("--keep", action="store_true", help="keep work directories")
    parser.add_argument(
        "--public",
        action="store_true",
        help=f"ripgrep {RIPGREP_TAG}: cold, clean, second checkout, one-edit",
    )
    args = parser.parse_args()
    if args.quick:
        args.crates = 8
        args.samples = 1
    if args.crates < 4:
        raise SystemExit("--crates must be at least 4")

    root = Path(__file__).resolve().parent.parent
    stock = str(Path(args.stock).absolute()) if args.stock else find_stock()
    if not stock:
        raise SystemExit("no stock cargo found; pass --stock")
    if args.artificer:
        artificer = Path(args.artificer).absolute()
    else:
        artificer = root / "target" / "release" / f"artificer{EXE}"
        print("building artificer (release) ...", flush=True)
        run([stock, "build", "--release", "--locked"], root, dict(os.environ))

    work = root / "target" / "bench-work"
    shutil.rmtree(work, ignore_errors=True)
    work.mkdir(parents=True)
    drivers = [("stock", str(stock)), ("artificer", str(artificer))]
    results = {name: {} for name, _ in drivers}
    for sample in range(args.samples):
        order = drivers if sample % 2 == 0 else list(reversed(drivers))
        for name, driver in order:
            print(f"sample {sample + 1}/{args.samples}: {name}", flush=True)
            if args.public:
                got = measure_public(driver, stock, work, 1, args.keep)
            else:
                got = measure(driver, stock, work, 1, args.crates, args.keep)
            for case, values in got.items():
                results[name].setdefault(case, []).extend(values)

    header = machine_header(root, stock, artificer)
    if args.public:
        subject = f"ripgrep {RIPGREP_TAG}, cargo build --locked --offline"
        cases = PUBLIC_CASES
    else:
        subject = f"generated workspace, {args.crates} crates"
        cases = GENERATED_CASES
    subject += f", {args.samples} sample(s)"
    if args.quick:
        subject += ", smoke run (not a measurement)"
    report = {"subject": subject, "header": header, "samples": results}
    (root / "target" / "bench-results.json").write_text(json.dumps(report, indent=2))

    print()
    print(subject)
    for line in header:
        print(line)
    print()
    print("| case | stock (s) | artificer (s) |")
    print("| --- | --- | --- |")
    for case in cases:
        if case not in results["stock"] or case not in results["artificer"]:
            continue
        print(f"| {case} | {cell(results['stock'][case])} | {cell(results['artificer'][case])} |")
    print()
    print("Median seconds, with the min-max range. Every sample asserted the")
    print("built program's output and the store counters for its case, and")
    print("aborted on any fallback to real Cargo. Stock Cargo runs with its")
    print("defaults, including incremental compilation. Artificer runs test")
    print("binaries at the same time; Cargo runs them one at a time. Raw")
    print("samples: target/bench-results.json.")
    if not args.keep:
        shutil.rmtree(work, ignore_errors=True)


def cell(values):
    middle = statistics.median(values)
    if len(values) == 1:
        return f"{middle:.2f}"
    return f"{middle:.2f} ({min(values):.2f}-{max(values):.2f})"


if __name__ == "__main__":
    main()
