use super::*;
use crate::cargo::TargetKind;
use crate::schedule::MetaStage;

pub(crate) const DIAGNOSTICS: &str = "diagnostics";
const ABORTING: &str = "aborting due to";
const ERROR_LEVELS: [&str; 2] = ["error", "error: internal compiler error"];
const PATH_MARKERS: [&str; 2] = ["--> ", "::: "];
const REMAPPED: [&str; 2] = ["./", ".\\"];

fn unit_label(target: &cargo::Target, harness: bool) -> String {
    let name = &target.name;
    let base = match TargetKind::of(target) {
        TargetKind::Lib => "lib".to_string(),
        TargetKind::Bin => format!("bin \"{name}\""),
        TargetKind::Test => return format!("test \"{name}\""),
        TargetKind::Example => format!("example \"{name}\""),
        TargetKind::Bench => return format!("bench \"{name}\""),
        TargetKind::BuildScript => return "build script".to_string(),
    };
    if harness {
        format!("{base} test")
    } else {
        base
    }
}

fn failure(pkg: &Package, target: &cargo::Target, cmd: &Command, errors: usize) -> String {
    let harness = cmd.get_args().any(|arg| arg == "--test");
    let label = unit_label(target, harness);
    let head = format!("could not compile `{}` ({label})", pkg.name);
    match errors {
        0 => head,
        1 => format!("{head} due to 1 previous error"),
        n => format!("{head} due to {n} previous errors"),
    }
}

fn source_prefix(sess: &Session, pkg: &Package) -> String {
    let root = pkg.root();
    let shown = root
        .strip_prefix(&sess.settings.workspace_root)
        .unwrap_or(root);
    if shown.as_os_str().is_empty() {
        String::new()
    } else {
        format!("{}{}", shown.display(), std::path::MAIN_SEPARATOR)
    }
}

pub(crate) struct Early {
    pub(crate) rmeta: PathBuf,
    pub(crate) early_rmeta: PathBuf,
    pub(crate) artifact: Artifact,
}

#[derive(serde::Deserialize)]
struct Notice {
    artifact: PathBuf,
    emit: String,
}

const ARTIFACT_NOTICE: &[u8] = b"{\"$message_type\":\"artifact\"";
const METADATA_EMIT: &str = "metadata";
const EARLY_METADATA_EMIT: &str = "early-metadata";

#[derive(Debug, PartialEq)]
enum Emitted {
    Metadata(PathBuf),
    EarlyMetadata(PathBuf),
    Other,
}

fn artifact_notice(line: &[u8]) -> Option<Emitted> {
    if !line.starts_with(ARTIFACT_NOTICE) {
        return None;
    }
    let notice: Notice = serde_json::from_slice(line).ok()?;
    Some(match notice.emit.as_str() {
        METADATA_EMIT => Emitted::Metadata(notice.artifact),
        EARLY_METADATA_EMIT => Emitted::EarlyMetadata(notice.artifact),
        _ => Emitted::Other,
    })
}

fn stream(
    cmd: &mut Command,
    sess: &Session,
    pkg: &Package,
    file: File,
    early: Option<&Early>,
) -> std::io::Result<std::process::ExitStatus> {
    use std::io::Write;
    let signal = early.and_then(|_| crate::schedule::signal());
    let mut writer = std::io::BufWriter::new(file);
    let status = crate::profile::status_lines(cmd, |line| {
        let Some(notice) = artifact_notice(line) else {
            writer.write_all(line)?;
            return writer.write_all(b"\n");
        };
        let ready = early.and_then(|early| match notice {
            Emitted::Metadata(path) if path.file_name() == early.rmeta.file_name() => {
                Some((early, MetaStage::Full, path, &early.rmeta))
            }
            Emitted::EarlyMetadata(path) if path.file_name() == early.early_rmeta.file_name() => {
                Some((early, MetaStage::Early, path, &early.early_rmeta))
            }
            _ => None,
        });
        if let Some((early, stage, emitted, published)) = ready {
            if emitted != *published {
                super::staging::publish_file(&emitted, published).map_err(std::io::Error::other)?;
            }
            sess.put(pkg.id.clone(), early.artifact.clone());
            if let Some(signal) = &signal {
                signal.metadata_ready(stage);
            }
        }
        Ok(())
    })?;
    writer.flush()?;
    Ok(status)
}

pub(crate) fn run_rustc(
    cmd: &mut Command,
    sess: &Session,
    pkg: &Package,
    target: &cargo::Target,
    out: &Path,
    early: Option<&Early>,
) -> Result<()> {
    let (_permit, _token) = crate::profile::span(crate::profile::UnitPhase::Permit, || {
        let permit = crate::jobs::acquire(&sess.settings.home)?;
        let token = sess
            .build_pool
            .as_ref()
            .map(|pool| pool.take())
            .transpose()?;
        anyhow::Ok((permit, token))
    })?;
    match &sess.build_pool {
        Some(pool) => pool.configure(cmd),
        None => crate::jobs::isolate(cmd),
    }
    crate::profile::span(crate::profile::ProcessPhase::Rustc, || {
        run_rustc_inner(cmd, sess, pkg, target, out, early)
    })
}

pub(crate) fn run_rustc_inner(
    cmd: &mut Command,
    sess: &Session,
    pkg: &Package,
    target: &cargo::Target,
    out: &Path,
    early: Option<&Early>,
) -> Result<()> {
    primary_env(cmd, sess, pkg);
    cmd.args([
        "--error-format=json",
        "--json=diagnostic-rendered-ansi,artifacts",
    ]);
    if std::env::var_os("ARTIFICER_TRACE").is_some() || crate::out::trace() {
        crate::out::diag(format!("ARTIFICER_CMD {}: {:?}", pkg.name, cmd));
    }
    let staged = super::staging::stage(&sess.settings.home, cmd, out)?;
    let path = out.join(DIAGNOSTICS);
    let file = File::create(&path).with_context(|| format!("create {}", path.display()))?;
    sess.announce(pkg, TargetKind::of(target) == TargetKind::BuildScript);
    let passes = crate::profile::passes_enabled();
    if passes {
        cmd.args(crate::profile::FLAGS);
        cmd.env(crate::profile::BOOTSTRAP.0, crate::profile::BOOTSTRAP.1);
    }
    let status = stream(cmd, sess, pkg, file, early).context("rustc")?;
    if let Some(staged) = &staged
        && status.success()
    {
        staged.publish(out)?;
    }
    if passes {
        crate::profile::note_passes(
            crate::profile::harvest(&path).with_context(|| format!("read {}", path.display()))?,
        );
    }
    let bytes = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
    let errors = show(sess, pkg, target, &bytes);
    if !status.success() {
        bail!(failure(pkg, target, cmd, errors));
    }
    crate::inputs::record(&sess.settings.home, out, pkg.root(), cmd)?;
    Ok(())
}

pub(crate) fn primary_env(cmd: &mut Command, sess: &Session, pkg: &Package) {
    if sess.primary.contains(&pkg.id) {
        cmd.env("CARGO_PRIMARY_PACKAGE", "1");
    } else {
        cmd.env_remove("CARGO_PRIMARY_PACKAGE");
    }
}

pub(crate) fn replay(sess: &Session, pkg: &Package, target: &cargo::Target, dir: &Path) {
    if let Ok(bytes) = std::fs::read(dir.join(DIAGNOSTICS)) {
        show(sess, pkg, target, &bytes);
    }
}

fn show(sess: &Session, pkg: &Package, target: &cargo::Target, bytes: &[u8]) -> usize {
    let color = crate::out::color();
    let prefix = source_prefix(sess, pkg);
    let root = format!("{}/", pkg.root().display());
    let mut errors = 0;
    for line in String::from_utf8_lossy(bytes).lines() {
        let Ok(mut msg) = serde_json::from_str::<Map<String, Value>>(line) else {
            crate::out::diag(line);
            continue;
        };
        let level = msg.get("level").and_then(Value::as_str).unwrap_or_default();
        let text = msg
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if ERROR_LEVELS.contains(&level) {
            if text.starts_with(ABORTING) {
                continue;
            }
            errors += 1;
        }
        rebase_spans(&mut msg, &prefix, &root);
        let rendered = msg
            .get("rendered")
            .and_then(Value::as_str)
            .map(|text| rebase_rendered(text, &prefix, &root));
        if sess.json {
            if let Some(text) = rendered {
                msg.insert("rendered".to_string(), Value::String(plain(&text)));
            }
            emit(pkg, target, Value::Object(msg));
        } else if let Some(text) = rendered
            && sess.first_sight(&text)
        {
            let text = if color { text } else { plain(&text) };
            crate::out::diag(text.strip_suffix('\n').unwrap_or(&text));
        }
    }
    errors
}

fn strip_source<'t>(text: &'t str, root: &str) -> Option<&'t str> {
    REMAPPED
        .iter()
        .find_map(|remapped| text.strip_prefix(remapped))
        .or_else(|| text.strip_prefix(root))
}

fn rebase_spans(msg: &mut Map<String, Value>, prefix: &str, root: &str) {
    if let Some(Value::Array(spans)) = msg.get_mut("spans") {
        for span in spans.iter_mut().filter_map(Value::as_object_mut) {
            if let Some(Value::String(file)) = span.get_mut("file_name")
                && let Some(rest) = strip_source(file, root)
            {
                *file = format!("{prefix}{rest}");
            }
        }
    }
    if let Some(Value::Array(children)) = msg.get_mut("children") {
        for child in children.iter_mut().filter_map(Value::as_object_mut) {
            rebase_spans(child, prefix, root);
        }
    }
}

fn rebase_rendered(text: &str, prefix: &str, root: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some((at, marker)) = PATH_MARKERS
        .iter()
        .filter_map(|marker| rest.find(marker).map(|at| (at, *marker)))
        .min()
    {
        let after = at + marker.len();
        out.push_str(&rest[..after]);
        rest = &rest[after..];
        let escapes = escape_run(rest);
        out.push_str(&rest[..escapes]);
        rest = &rest[escapes..];
        if let Some(tail) = strip_source(rest, root) {
            out.push_str(prefix);
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

fn escape_run(text: &str) -> usize {
    let mut len = 0;
    while text[len..].starts_with('\u{1b}') {
        match text[len..].find(|c: char| c.is_ascii_alphabetic()) {
            Some(end) => len += end + 1,
            None => break,
        }
    }
    len
}

fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        for end in chars.by_ref() {
            if end.is_ascii_alphabetic() {
                break;
            }
        }
    }
    out
}

fn emit(pkg: &Package, target: &cargo::Target, msg: Value) {
    crate::out::out(serde_json::json!({
        "reason": "compiler-message",
        "package_id": pkg.id,
        "manifest_path": pkg.manifest_path,
        "target": {
            "kind": target.kind,
            "crate_types": target.crate_types,
            "name": target.name,
            "src_path": target.src_path,
            "edition": target.edition,
            "doc": target.doc,
            "doctest": target.doctest,
            "test": target.test
        },
        "message": msg
    }));
}

pub(crate) fn note_rustc(home: &Path) {
    if !home.join("COUNT_RUSTC").is_file() {
        return;
    }
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(home.join("rustc-runs"))
    {
        drop(writeln!(f, "ran"));
    }
}

#[cfg(test)]
#[path = "diagnostics_tests.rs"]
mod tests;
