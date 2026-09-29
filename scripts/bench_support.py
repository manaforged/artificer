import os
import platform
import shutil
import subprocess
import time
from pathlib import Path


EXE = ".exe" if os.name == "nt" else ""


def run(cmd, cwd, env):
    """Run one command; raise with the tail of stderr on failure."""
    done = subprocess.run(
        [str(c) for c in cmd],
        cwd=str(cwd),
        env=env,
        capture_output=True,
        text=True,
        timeout=1800,
    )
    if done.returncode != 0:
        tail = "\n".join((done.stderr or done.stdout).splitlines()[-20:])
        raise SystemExit(f"command failed: {' '.join(map(str, cmd))}\n{tail}")
    return done


def timed(cmd, cwd, env):
    start = time.perf_counter()
    run(cmd, cwd, env)
    return time.perf_counter() - start


def read_counters(store):
    def one(name):
        path = store / name
        if not path.is_file():
            return 0
        return int(path.read_text().strip())

    return one("stat.hits"), one("stat.misses"), one("stat.fallbacks")


def store_delta(case, before, after):
    """Hits and misses for one case. A fallback to real Cargo aborts the
    run: its time would be stock Cargo's time under Artificer's name."""
    fallbacks = after[2] - before[2]
    if fallbacks:
        raise SystemExit(f"{case}: {fallbacks} command(s) fell back to real Cargo")
    return after[0] - before[0], after[1] - before[1]


def check_counters(is_artificer, case, before, after):
    """The wall time is only meaningful when the store did what the case
    claims. Cold must compile, warm cases must only hit, edit must
    recompile at least one unit, and no case may fall back."""
    if not is_artificer:
        return
    hits, misses = store_delta(case, before, after)
    shape_ok = {
        "cold": hits == 0 and misses > 0,
        "noop": hits > 0 and misses == 0,
        "second-checkout": hits > 0 and misses == 0,
        "edit": misses > 0,
        "test": misses > 0,
        "test-warm": hits > 0 and misses == 0,
        "nextest": True,
        "nextest-warm": hits > 0 and misses == 0,
        "check": True,
    }[case]
    if not shape_ok:
        raise SystemExit(
            f"{case}: store counters moved hits+{hits} misses+{misses},"
            " which is not what the case measures"
        )


def nextest_bin():
    return shutil.which("cargo-nextest")


def cargo_named(driver, work):
    """A `cargo` name for the driver, created inside the bench work
    directory so it never replaces a real Cargo next to the binary."""
    driver = Path(driver)
    if driver.stem.lower() == "cargo":
        return driver
    work.mkdir(parents=True, exist_ok=True)
    shim = work / f"cargo{EXE}"
    if shim.exists() or shim.is_symlink():
        shim.unlink()
    try:
        shim.symlink_to(driver)
    except OSError:
        shutil.copy2(driver, shim)
    return shim


def driver_env(driver, stock, target, store):
    env = dict(os.environ)
    compiler = Path(stock).with_name(f"rustc{EXE}")
    if compiler.is_file():
        env.setdefault("RUSTC", str(compiler))
    env["CARGO_TARGET_DIR"] = str(target)
    env.pop("RUSTC_WRAPPER", None)
    env.pop("RUSTC_WORKSPACE_WRAPPER", None)
    if driver == stock:
        env.pop("ARTIFICER_HOME", None)
    else:
        env["ARTIFICER_HOME"] = str(store)
        env["ARTIFICER_NOSERVE"] = "1"
    return env


def run_tests(driver, stock, checkout, target, store, kind, nextest):
    env = driver_env(driver, stock, target, store)
    if kind.startswith("nextest"):
        cmd = [nextest, "nextest", "run", "--workspace"]
        env["CARGO"] = str(cargo_named(driver, Path(store).parent / "cargo-name"))
    elif kind == "check":
        cmd = [driver, "check", "--workspace"]
    else:
        cmd = [driver, "test", "--workspace"]
    return timed(cmd, checkout, env)


def find_stock():
    """The real Cargo, not the Artificer shim that may win PATH."""
    found = None
    try:
        done = subprocess.run(
            ["rustup", "which", "cargo"], capture_output=True, text=True, timeout=60
        )
        if done.returncode == 0 and done.stdout.strip():
            found = done.stdout.strip()
    except OSError:
        pass
    found = found or shutil.which("cargo")
    if found and ".artificer" in Path(found).resolve().parts:
        raise SystemExit(f"{found} is the Artificer shim; pass --stock")
    return found


def output_of(cmd, cwd=None):
    try:
        done = subprocess.run(
            [str(c) for c in cmd], cwd=cwd, capture_output=True, text=True, timeout=60
        )
    except OSError:
        return ""
    return done.stdout.strip() if done.returncode == 0 else ""


def machine_header(root, stock, artificer):
    """Everything a reader needs to reproduce or discount a number."""
    rustc = Path(stock).with_name(f"rustc{EXE}")
    sysctl = shutil.which("sysctl") or "/usr/sbin/sysctl"
    cpu = output_of([sysctl, "-n", "machdep.cpu.brand_string"])
    if not cpu and Path("/proc/cpuinfo").is_file():
        cpu = next(
            (
                line.split(":", 1)[1].strip()
                for line in Path("/proc/cpuinfo").read_text().splitlines()
                if line.startswith("model name")
            ),
            "",
        )
    commit = output_of(["git", "rev-parse", "HEAD"], root)
    dirty = output_of(["git", "status", "--porcelain"], root)
    return [
        f"host: {platform.platform()}, {cpu or platform.processor()}, {os.cpu_count()} cpus",
        f"artificer: {output_of([artificer, '--version'])} at {commit}"
        + (" (dirty tree)" if dirty else ""),
        f"cargo: {output_of([stock, '-V'])}",
        f"rustc: {output_of([rustc if rustc.is_file() else 'rustc', '-V'])}",
    ]
