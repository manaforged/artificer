use super::*;
use crate::build::{Kind, Mode, TargetSel};

pub fn check_extras(
    sess: &Session,
    meta: &cargo::Metadata,
    id: &str,
    sel: &TargetSel,
) -> Result<()> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let feats = &node.features;
    let bin_exe = bin_exes(sess, pkg, feats);
    let has_lib = pkg.lib_target().is_some();
    let main = pkg
        .bin_target()
        .filter(|_| !has_lib)
        .map(|t| t.name.as_str());
    for (t, kind, mode) in sel.units(pkg, feats)? {
        let Some((scan, self_extern, with_exe)) =
            scan_for(kind, mode, main == Some(t.name.as_str()), has_lib)
        else {
            continue;
        };
        let exe: &[(String, PathBuf)] = if with_exe { &bin_exe } else { &[] };
        check_one(sess, pkg, node, t, scan, self_extern, exe)?;
    }
    Ok(())
}

fn bin_exes(sess: &Session, pkg: &Package, feats: &[String]) -> Vec<(String, PathBuf)> {
    pkg.targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin") && Package::covered(t, feats))
        .map(|t| {
            (
                format!("CARGO_BIN_EXE_{}", t.name),
                sess.settings
                    .profile_dir()
                    .join(artifact::bin_name(&t.name)),
            )
        })
        .collect()
}

fn scan_for(kind: Kind, mode: Mode, main_bin: bool, has_lib: bool) -> Option<(Scan, bool, bool)> {
    match (kind, mode) {
        (Kind::Lib, Mode::Normal) => None,
        (Kind::Bin, Mode::Normal) if main_bin => None,
        (Kind::Bin, Mode::Normal) => Some((Scan::Bin, has_lib, false)),
        (Kind::Example, Mode::Normal) => Some((Scan::Example, has_lib, true)),
        (Kind::Lib, Mode::Test) => Some((Scan::Test, false, true)),
        (Kind::Bin, Mode::Test) => Some((Scan::BinTest, has_lib, true)),
        (_, Mode::Test) => Some((Scan::Test, has_lib, true)),
        (_, Mode::Normal) => None,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scan {
    Bin,
    Example,
    Test,
    BinTest,
}

impl Scan {
    fn prefix(self) -> &'static str {
        match self {
            Scan::Bin => "scan-",
            Scan::Example => "example-scan-",
            Scan::Test => "test-scan-",
            Scan::BinTest => "test-scan-bin-",
        }
    }

    fn dev_deps(self) -> bool {
        self != Scan::Bin
    }

    fn harness(self) -> bool {
        matches!(self, Scan::Test | Scan::BinTest)
    }

    fn extern_set(self) -> invoke::ExternSet {
        if self.dev_deps() {
            invoke::ExternSet::Test
        } else {
            invoke::ExternSet::Lib
        }
    }
}

struct ScanUnit<'u> {
    pkg: &'u Package,
    node: &'u cargo::Node,
    target: &'u cargo::Target,
    scan: Scan,
    self_extern: bool,
}

fn check_one(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    target: &cargo::Target,
    scan: Scan,
    self_extern: bool,
    bin_exe: &[(String, PathBuf)],
) -> Result<()> {
    let unit = ScanUnit {
        pkg,
        node,
        target,
        scan,
        self_extern,
    };
    let crate_name = target.name.replace('-', "_");
    let target_tmpdir = invoke::uses_target_tmpdir(target);
    let harness = scan.harness() && crate::manifest::harness(&pkg.manifest_path, target);
    let marker = if scan.harness() && !harness {
        testing::NO_HARNESS
    } else {
        ""
    };
    let kind = format!("{}{crate_name}{marker}", scan.prefix());
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(|s| s.stamp.clone());
    let keyed = unit_key::unit_digest(
        sess,
        pkg,
        node,
        &kind,
        &node.features,
        &[],
        true,
        stamp.as_deref(),
        target_tmpdir,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &keyed.digest)?
        .lineage(keyed.lineage);
    let mut cmd = sess.settings.rustc_cmd(pkg);
    cmd.arg("--emit=dep-info,metadata");
    if harness {
        cmd.arg("--test");
    } else if scan.harness() {
        cmd.args(["--cfg", "test"]);
    }
    cmd.envs(bin_exe.iter().map(|(name, path)| (name, path)));
    invoke::set_target_tmpdir(&mut cmd, sess, target_tmpdir);
    let cmd = invoke::rustc_base(
        cmd,
        sess,
        pkg,
        target,
        false,
        &node.features,
        &action.out,
        script.as_ref(),
        false,
    );
    let cmd = scan_args(sess, &unit, cmd)?;
    let manifest = unit_key::dep_manifest(
        sess,
        node,
        scan.dev_deps(),
        self_extern.then_some(pkg.id.as_str()),
    )?;
    settle(sess, &unit, action, cmd, &manifest)
}

fn scan_args(sess: &Session, unit: &ScanUnit, mut cmd: Command) -> Result<Command> {
    let pkg = unit.pkg;
    invoke::add_externs(&mut cmd, sess, unit.node, unit.scan.extern_set(), true)?;
    invoke::add_natives(&mut cmd, sess, &pkg.id, true);
    if unit.self_extern
        && let Some(art) = sess.get(&pkg.id)
    {
        let path = art.rmeta.as_ref().unwrap_or(&art.path);
        cmd.arg("--extern")
            .arg(format!("{}={}", art.crate_name, path.display()));
    }
    cmd.arg(&unit.target.src_path);
    invoke::primary_env(&mut cmd, sess, pkg);
    Ok(cmd)
}

fn settle(
    sess: &Session,
    unit: &ScanUnit,
    action: action::Action,
    mut cmd: Command,
    manifest: &str,
) -> Result<()> {
    let (pkg, target) = (unit.pkg, unit.target);
    let slot = &action.slot;
    if slot.hit() {
        if unit_key::deps_match(&slot.out_dir(), manifest)
            && crate::inputs::matches(&sess.settings.home, &slot.out_dir(), pkg.root(), &cmd)
        {
            sess.retain(action.lease()?);
            invoke::replay(sess, pkg, target, &slot.out_dir());
            return Ok(());
        }
        action.invalidate()?;
    }
    let out = action.out.clone();
    action.prepare()?;
    std::fs::create_dir_all(&out)?;
    invoke::note_rustc(&sess.settings.home);
    invoke::run_rustc(&mut cmd, sess, pkg, target, &out, None)?;
    std::fs::write(out.join(unit_key::DEPS_FILE), manifest)?;
    action.finish()?;
    sess.retain(action.lease()?);
    Ok(())
}

pub fn doctest_cmd(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    lib: &cargo::Target,
    args: &[String],
) -> Result<Option<Command>> {
    if pkg.is_proc_macro() || !lib.doctest {
        return Ok(None);
    }
    let Some(art) = sess.get(&pkg.id) else {
        bail!("missing lib artifact for {}", pkg.id);
    };
    let crate_name = lib.name.replace('-', "_");
    let rustdoc = std::env::var("RUSTDOC")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "rustdoc".to_string());
    let mut cmd = Command::new(rustdoc);
    sess.settings.apply_env(&mut cmd);
    cmd.current_dir(&sess.settings.toolchain_dir);
    cmd.arg("--test")
        .arg(&lib.src_path)
        .arg("--crate-name")
        .arg(&crate_name)
        .arg("--edition")
        .arg(&lib.edition)
        .arg("--extern")
        .arg(format!("{crate_name}={}", art.path.display()));
    for feat in &node.features {
        cmd.arg("--cfg").arg(format!("feature=\"{feat}\""));
    }
    for a in invoke::check_cfg_args(pkg) {
        cmd.arg("--check-cfg").arg(a);
    }
    for a in sess.settings.lints(pkg).iter() {
        cmd.arg(a);
    }
    for a in &sess.settings.rustflags {
        cmd.arg(a);
    }
    for a in settings::rustdocflags() {
        cmd.arg(a);
    }
    cmd.env("CARGO_CRATE_NAME", &crate_name);
    cargo::set_package_env(&mut cmd, pkg);
    if let Some(s) = ensure_script(sess, pkg, node)? {
        invoke::apply_script(&mut cmd, &s);
    }
    invoke::add_externs(&mut cmd, sess, node, invoke::ExternSet::Test, false)?;
    for a in args {
        cmd.arg("--test-args").arg(a);
    }
    Ok(Some(cmd))
}
