import os
import shutil
from bench_generated import build_command
from bench_support import EXE, check_counters, read_counters, run, store_delta, timed
from pathlib import Path


RIPGREP_TAG = "14.1.1"

RIPGREP_URL = "https://github.com/BurntSushi/ripgrep.git"


def fetch_ripgrep(dest, stock):
    if (dest / "Cargo.toml").is_file() and (dest / "Cargo.lock").is_file():
        return dest
    if dest.exists():
        shutil.rmtree(dest)
    dest.parent.mkdir(parents=True, exist_ok=True)
    run(
        [
            "git",
            "clone",
            "--depth",
            "1",
            "--branch",
            RIPGREP_TAG,
            RIPGREP_URL,
            dest,
        ],
        dest.parent,
        dict(os.environ),
    )
    run([stock, "fetch", "--locked"], dest, dict(os.environ))
    return dest


def rg_version(target, checkout):
    app = target / "debug" / f"rg{EXE}"
    if not app.is_file():
        raise SystemExit(f"ripgrep did not deliver {app}")
    done = run([app, "--version"], checkout, dict(os.environ))
    got = done.stdout.strip()
    if RIPGREP_TAG not in got:
        raise SystemExit(f"rg --version printed {got!r}, expected {RIPGREP_TAG}")
    return got


EDIT_MARKER = "artificer-bench-edit"


def patch_one_edit(checkout):
    """Change what `rg --version` prints, so the check below proves the
    edit reached the binary."""
    path = checkout / "crates" / "core" / "flags" / "doc" / "version.rs"
    text = path.read_text() if path.is_file() else ""
    old = 'format!("ripgrep {digits}")'
    if old not in text:
        raise SystemExit(f"{path} no longer contains {old}")
    path.write_text(text.replace(old, f'format!("ripgrep {{digits}} {EDIT_MARKER}")'))
    return path, text


def measure_public(driver, stock, work, samples, keep):
    """Raw seconds per case, one entry per sample."""
    src = fetch_ripgrep(work / "ripgrep-src", stock)
    times = {"cold": [], "clean": [], "second-checkout": [], "one-edit": []}
    is_artificer = Path(driver).resolve() != Path(stock).resolve()
    for sample in range(samples):
        sample_dir = work / f"public-{Path(driver).stem}-{sample}"
        shutil.rmtree(sample_dir, ignore_errors=True)
        checkout_a = sample_dir / "checkout-a"
        shutil.copytree(src, checkout_a, symlinks=True)
        target_a = sample_dir / "target-a"
        store = sample_dir / "store"
        store.mkdir(parents=True)
        cmd, env = build_command(driver, stock, target_a, store)
        cmd = [cmd[0], "build", "--locked", "--offline"]
        before = read_counters(store)
        times["cold"].append(timed(cmd, checkout_a, env))
        expected = rg_version(target_a, checkout_a)
        check_counters(is_artificer, "cold", before, read_counters(store))

        edited, before_edit = patch_one_edit(checkout_a)
        before = read_counters(store)
        times["one-edit"].append(timed(cmd, checkout_a, env))
        if EDIT_MARKER not in rg_version(target_a, checkout_a):
            raise SystemExit("one-edit rebuild did not reach the rg binary")
        if is_artificer:
            hits, misses = store_delta("one-edit", before, read_counters(store))
            if hits == 0 or misses == 0:
                raise SystemExit(
                    f"one-edit: hits+{hits} misses+{misses}; expected reused deps and a miss"
                )
        edited.write_text(before_edit)

        shutil.rmtree(target_a, ignore_errors=True)
        before = read_counters(store)
        times["clean"].append(timed(cmd, checkout_a, env))
        if rg_version(target_a, checkout_a) != expected:
            raise SystemExit("clean rebuild changed rg --version")
        if is_artificer:
            hits, misses = store_delta("clean", before, read_counters(store))
            if hits == 0 or misses:
                raise SystemExit(f"clean: hits+{hits} misses+{misses}; expected only hits")

        checkout_b = sample_dir / "checkout-b"
        shutil.copytree(src, checkout_b, symlinks=True)
        target_b = sample_dir / "target-b"
        cmd_b, env_b = build_command(driver, stock, target_b, store)
        cmd_b = [cmd_b[0], "build", "--locked", "--offline"]
        before = read_counters(store)
        times["second-checkout"].append(timed(cmd_b, checkout_b, env_b))
        if rg_version(target_b, checkout_b) != expected:
            raise SystemExit("second checkout changed rg --version")
        if is_artificer:
            hits, misses = store_delta("second-checkout", before, read_counters(store))
            if hits == 0 or misses:
                raise SystemExit(
                    f"second-checkout: hits+{hits} misses+{misses}; expected only hits"
                )
        if not keep:
            shutil.rmtree(sample_dir, ignore_errors=True)
    return times
