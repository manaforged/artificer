use super::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Ok,
    Bad,
    Note,
}

impl Level {
    fn check(ok: bool) -> Self {
        if ok { Self::Ok } else { Self::Bad }
    }

    fn report(ok: bool) -> Self {
        if ok { Self::Ok } else { Self::Note }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Ok => "ok  ",
            Self::Bad => "BAD ",
            Self::Note => "note",
        }
    }
}

struct Row(Level, &'static str, String);

pub fn doctor(home: &Path) -> Result<i32> {
    let mut bad = 0u32;
    let mut emit = |Row(level, name, note): Row| {
        println!("{} {:<10} {}", level.tag(), name, note);
        bad += u32::from(level == Level::Bad);
    };
    emit(mode_row(home)?);
    emit(shim_row());
    emit(real_cargo_row());
    emit(Row(
        Level::check(ready(home)),
        "store-path",
        home.display().to_string(),
    ));
    emit(jobserver_row(home));
    emit(store_row(home)?);
    emit(rustc_row(home));
    emit(fallbacks_row(home));
    emit(serve_row(home));
    emit(analyzer_row());
    Ok(if bad == 0 { 0 } else { 1 })
}

fn mode_row(home: &Path) -> Result<Row> {
    let mode = if crate::mods::enabled(home)? {
        "enabled"
    } else {
        "disabled; shim commands use Cargo"
    };
    Ok(Row(Level::Note, "mode", mode.to_string()))
}

fn shim_row() -> Row {
    let cargo_name = format!("cargo{}", std::env::consts::EXE_SUFFIX);
    let shim = dirs_home().join(".artificer").join("bin").join(&cargo_name);
    let first = crate::platform::on_path("cargo");
    let note = match &first {
        Some(p) if *p == shim => p.display().to_string(),
        Some(p) => format!("{} wins PATH; evaluate `artificer env`", p.display()),
        None => "no cargo on PATH".into(),
    };
    Row(Level::check(first.as_ref() == Some(&shim)), "shim", note)
}

fn real_cargo_row() -> Row {
    let stamp = dirs_home().join(".artificer").join("real-cargo");
    let real = std::fs::read_to_string(&stamp)
        .ok()
        .map(|s| s.trim().to_string());
    let pinned = real
        .as_deref()
        .is_some_and(|p| cargo::pinned_toolchain_cargo(Path::new(p)));
    let ok = real.as_deref().is_some_and(|p| Path::new(p).is_file()) && !pinned;
    let note = match &real {
        Some(p) if pinned => format!("{p} is a pinned toolchain — rust-toolchain.toml is ignored"),
        Some(p) => p.clone(),
        None => format!("no {}", stamp.display()),
    };
    Row(Level::check(ok), "real-cargo", note)
}

#[cfg(unix)]
fn jobserver_row(home: &Path) -> Row {
    match jobs::install(home).and_then(|()| jobs::pool_depth(&jobs::fifo(home))) {
        Ok(n) => Row(Level::Ok, "jobserver", format!("{n} available tokens")),
        Err(e) => Row(Level::Bad, "jobserver", format!("{e}")),
    }
}

#[cfg(not(unix))]
fn jobserver_row(_home: &Path) -> Row {
    Row(Level::Note, "jobserver", "native Cargo scheduler".into())
}

fn store_row(home: &Path) -> Result<Row> {
    let stat = store_stat(home)?;
    let cap = store_cap(home)?;
    Ok(Row(
        Level::check(stat.bytes <= cap),
        "store",
        format!(
            "{} units, {:.1} MB (cap {} GB, `artificer clean` or daily gc)",
            stat.units,
            stat.bytes as f64 / 1_048_576.0,
            cap >> 30
        ),
    ))
}

fn rustc_row(home: &Path) -> Row {
    match key::rustc_version(home) {
        Ok(v) => Row(
            Level::Ok,
            "rustc",
            v.lines().next().unwrap_or_default().to_string(),
        ),
        Err(e) => Row(Level::Bad, "rustc", format!("{e}")),
    }
}

fn fallbacks_row(home: &Path) -> Row {
    let (fallbacks, last) = store::fallbacks(home);
    let note = if fallbacks == 0 {
        "none".to_string()
    } else {
        format!("{fallbacks} (last: {})", last.unwrap_or_default())
    };
    Row(Level::report(fallbacks == 0), "fallbacks", note)
}

fn serve_row(home: &Path) -> Row {
    let up = serve_ping(home);
    let note = if up {
        "daemon up"
    } else {
        "down; in-process path"
    };
    Row(Level::report(up), "serve", note.to_string())
}

fn analyzer_row() -> Row {
    let ra = dirs_home()
        .join(".config")
        .join("rust-analyzer")
        .join("rust-analyzer.toml");
    let wired = std::fs::read_to_string(&ra).is_ok_and(|s| analyzer_wired(&s));
    let note = if wired {
        "CLI config points at the shim".to_string()
    } else {
        format!(
            "CLI config not wired ({}); editors set CARGO in their own settings",
            ra.display()
        )
    };
    Row(Level::report(wired), "analyzer", note)
}

pub(super) fn analyzer_wired(config: &str) -> bool {
    let shim = resolve_path(
        &dirs_home()
            .join(".artificer")
            .join("bin")
            .join(format!("cargo{}", std::env::consts::EXE_SUFFIX)),
    );
    let Ok(doc) = config.parse::<toml::Value>() else {
        return config.contains(".artificer");
    };
    doc.get("cargo")
        .and_then(|c| c.get("extraEnv"))
        .and_then(|e| e.get("CARGO"))
        .and_then(toml::Value::as_str)
        .is_some_and(|p| resolve_path(Path::new(p)) == shim)
}
