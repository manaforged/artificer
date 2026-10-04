use super::*;

pub fn check_extras(
    sess: &Session,
    meta: &cargo::Metadata,
    id: &str,
    tests: bool,
    all: bool,
) -> Result<()> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let feats = &node.features;
    let bin_exe: Vec<(String, PathBuf)> = pkg
        .targets
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
        .collect();
    let skip = usize::from(pkg.lib_target().is_none());
    let self_extern = pkg.lib_target().is_some();
    for t in pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"))
        .skip(skip)
    {
        if Package::covered(t, feats) {
            check_one(sess, pkg, node, t, Scan::Bin, self_extern, &[])?;
        }
    }
    if tests || all {
        if let Some(lib) = pkg.lib_target()
            && !pkg.is_proc_macro()
            && lib.test
        {
            check_one(sess, pkg, node, lib, Scan::Test, false, &bin_exe)?;
        }
        for t in pkg
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "test"))
        {
            if Package::covered(t, feats) {
                check_one(sess, pkg, node, t, Scan::Test, true, &bin_exe)?;
            }
        }
    }
    if all {
        for t in pkg
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "example"))
        {
            if Package::covered(t, feats) {
                check_one(sess, pkg, node, t, Scan::Example, true, &bin_exe)?;
            }
        }
        for t in pkg
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bench"))
        {
            if Package::covered(t, feats) {
                check_one(sess, pkg, node, t, Scan::Test, true, &bin_exe)?;
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scan {
    Bin,
    Example,
    Test,
}

impl Scan {
    fn prefix(self) -> &'static str {
        match self {
            Scan::Bin => "scan-",
            Scan::Example => "example-scan-",
            Scan::Test => "test-scan-",
        }
    }

    fn dev_deps(self) -> bool {
        self != Scan::Bin
    }
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
    let crate_name = target.name.replace('-', "_");
    let target_tmpdir = invoke::uses_target_tmpdir(target);
    let prefix = scan.prefix();
    let kind = format!("{prefix}{crate_name}");
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
    let digest = keyed.digest;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &digest)?
        .lineage(keyed.lineage);
    let slot = &action.slot;
    let out = action.out.clone();
    let mut cmd = sess.settings.rustc_cmd(pkg);
    cmd.arg("--emit=dep-info,metadata");
    if scan == Scan::Test {
        cmd.arg("--test");
    }
    cmd.envs(bin_exe.iter().map(|(name, path)| (name, path)));
    invoke::set_target_tmpdir(&mut cmd, sess, target_tmpdir);
    let mut cmd = invoke::rustc_base(
        cmd,
        sess,
        pkg,
        target,
        false,
        &node.features,
        &out,
        script.as_ref(),
        false,
    );
    invoke::add_externs(
        &mut cmd,
        sess,
        node,
        if scan.dev_deps() {
            invoke::ExternSet::Test
        } else {
            invoke::ExternSet::Lib
        },
        true,
    )?;
    invoke::add_natives(&mut cmd, sess, &pkg.id, true);
    if self_extern && let Some(art) = sess.get(&pkg.id) {
        let path = art.rmeta.as_ref().unwrap_or(&art.path);
        cmd.arg("--extern")
            .arg(format!("{}={}", art.crate_name, path.display()));
    }
    cmd.arg(&target.src_path);
    invoke::primary_env(&mut cmd, sess, pkg);
    let manifest = unit_key::dep_manifest(
        sess,
        node,
        scan.dev_deps(),
        self_extern.then_some(pkg.id.as_str()),
    )?;
    if slot.hit() {
        if unit_key::deps_match(&slot.out_dir(), &manifest)
            && crate::inputs::matches(&sess.settings.home, &slot.out_dir(), pkg.root(), &cmd)
        {
            sess.retain(action.lease()?);
            invoke::replay(sess, pkg, target, &slot.out_dir());
            return Ok(());
        }
        action.invalidate()?;
    }
    action.prepare()?;
    std::fs::create_dir_all(&out)?;
    invoke::note_rustc(&sess.settings.home);
    invoke::run_rustc(&mut cmd, sess, pkg, target, &out)?;
    std::fs::write(out.join(unit_key::DEPS_FILE), &manifest)?;
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
