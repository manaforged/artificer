use super::*;

pub struct TestBin {
    pub exe: PathBuf,
    pub target: cargo::Target,
    pub rustc: RustcOutcome,
}

#[derive(Debug, Default, Clone)]
pub struct TestSel {
    pub lib: bool,
    pub only: Vec<String>,
}

impl TestSel {
    fn wants_lib(&self) -> bool {
        self.only.is_empty()
    }

    fn wants_bins(&self) -> bool {
        !self.lib && self.only.is_empty()
    }

    fn wants(&self, name: &str) -> bool {
        !self.lib && (self.only.is_empty() || self.only.iter().any(|o| o == name))
    }
}

pub fn compile_tests(
    sess: &Session,
    meta: &cargo::Metadata,
    id: &str,
    sel: &TestSel,
) -> Result<Vec<TestBin>> {
    compile_pkg(sess, meta, id)?;
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let mut bin_exe: Vec<(String, PathBuf)> = Vec::new();
    for t in pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"))
    {
        if Package::covered(t, &node.features) {
            bin_exe.push((
                format!("CARGO_BIN_EXE_{}", t.name),
                compile_bin(sess, pkg, node, t)?,
            ));
        }
    }
    let mut bins = Vec::new();
    if sel.wants_lib()
        && let Some(lib) = pkg.lib_target()
        && !pkg.is_proc_macro()
    {
        bins.push(compile_test_one(
            sess, pkg, node, lib, "lib", true, &bin_exe,
        )?);
    }
    if sel.wants_bins() {
        for t in pkg
            .targets
            .iter()
            .filter(|t| t.kind.iter().any(|k| k == "bin") && t.test)
        {
            if Package::covered(t, &node.features) {
                bins.push(compile_test_one(
                    sess, pkg, node, t, "bin", false, &bin_exe,
                )?);
            }
        }
    }
    for t in pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "test") && sel.wants(&t.name))
    {
        if Package::covered(t, &node.features) {
            bins.push(compile_test_one(
                sess, pkg, node, t, "int", false, &bin_exe,
            )?);
        }
    }
    Ok(bins)
}

fn compile_test_one(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    target: &cargo::Target,
    label: &str,
    is_lib: bool,
    bin_exe: &[(String, PathBuf)],
) -> Result<TestBin> {
    let crate_name = target.name.replace('-', "_");
    let target_tmpdir = invoke::uses_target_tmpdir(target);
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(Script::stamp);
    let keyed = unit_key::unit_digest(
        sess,
        pkg,
        node,
        &format!(
            "test-{label}-{crate_name}-{}",
            bin_exe
                .iter()
                .map(|(k, v)| format!("{k}={}", v.display()))
                .collect::<Vec<_>>()
                .join(",")
        ),
        &node.features,
        &[],
        true,
        stamp.as_deref(),
        target_tmpdir,
    )?;
    let digest = keyed.digest;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Test, &digest)?
        .lineage(keyed.lineage);
    let slot = &action.slot;
    let out = action.out.clone();
    let exe = out.join(if cfg!(windows) {
        format!("{crate_name}.exe")
    } else {
        crate_name.clone()
    });

    let mut cmd = sess.settings.rustc_cmd(pkg);
    cmd.args(["--test", "--emit=dep-info,link", "--cfg", "test"]);
    cmd.envs(bin_exe.iter().map(|(name, path)| (name, path)));
    invoke::set_target_tmpdir(&mut cmd, sess, target_tmpdir);
    let mut cmd = invoke::rustc_base(
        cmd,
        sess,
        pkg,
        target,
        true,
        &node.features,
        &out,
        script.as_ref(),
        true,
    );
    cmd.args(["-C", "panic=unwind"]);
    invoke::add_externs(&mut cmd, sess, node, invoke::ExternSet::Test, false)?;
    invoke::add_natives(&mut cmd, sess);
    if !is_lib && let Some(art) = sess.get(&pkg.id) {
        cmd.arg("--extern")
            .arg(format!("{}={}", art.crate_name, art.path.display()));
    }
    cmd.arg(&target.src_path);
    invoke::primary_env(&mut cmd, sess, pkg);
    let manifest = unit_key::dep_manifest(sess, node, true, (!is_lib).then_some(pkg.id.as_str()))?;
    if slot.hit() {
        if !unit_key::deps_match(&out, &manifest)
            || !crate::inputs::matches(&sess.settings.home, &out, pkg.root(), &cmd)
        {
            action.invalidate()?;
        } else if exe.is_file() {
            sess.retain(action.lease()?);
            invoke::replay(sess, pkg, target, &out);
            return Ok(TestBin {
                exe,
                target: target.clone(),
                rustc: RustcOutcome::Restored,
            });
        }
    }

    action.prepare()?;
    std::fs::create_dir_all(&out)?;
    invoke::note_rustc(&sess.settings.home);
    invoke::run_rustc(&mut cmd, sess, pkg, target, &out)?;
    std::fs::write(out.join(unit_key::DEPS_FILE), &manifest)?;
    if !exe.is_file() {
        bail!("no test exe {crate_name} in {}", out.display());
    }
    action.finish()?;
    sess.retain(action.lease()?);
    Ok(TestBin {
        exe,
        target: target.clone(),
        rustc: RustcOutcome::Ran,
    })
}
