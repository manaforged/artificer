use super::*;

pub(super) const NO_HARNESS: &str = "-no-harness";

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
        !self.lib && (self.only.is_empty() || self.named(name))
    }

    fn named(&self, name: &str) -> bool {
        self.only.iter().any(|o| o == name)
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
    let bins: Vec<&cargo::Target> = pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin") && Package::covered(t, &node.features))
        .collect();
    let bin_exe = crate::schedule::fan_out(bins.clone(), |t| {
        Ok((
            format!("CARGO_BIN_EXE_{}", t.name),
            compile_bin(sess, pkg, node, t)?,
        ))
    })?;
    let mut wanted = Vec::new();
    if sel.wants_lib()
        && let Some(lib) = pkg.lib_target()
        && !pkg.is_proc_macro()
    {
        wanted.push((lib, "lib", true));
    }
    if sel.wants_bins() {
        wanted.extend(bins.iter().filter(|t| t.test).map(|t| (*t, "bin", false)));
    }
    wanted.extend(
        pkg.targets
            .iter()
            .filter(|t| {
                t.kind.iter().any(|k| k == "test")
                    && sel.wants(&t.name)
                    && (t.test || sel.named(&t.name))
                    && Package::covered(t, &node.features)
            })
            .map(|t| (t, "int", false)),
    );
    crate::schedule::fan_out(wanted, |(target, label, is_lib)| {
        compile_test_one(sess, pkg, node, target, label, is_lib, &bin_exe)
    })
}

struct TestUnit<'u> {
    sess: &'u Session,
    pkg: &'u Package,
    node: &'u cargo::Node,
    target: &'u cargo::Target,
    is_lib: bool,
    harness: bool,
    target_tmpdir: bool,
}

fn test_kind(
    label: &str,
    crate_name: &str,
    harness: bool,
    bin_exe: &[(String, PathBuf)],
) -> String {
    let marker = if harness { "" } else { NO_HARNESS };
    let exes: Vec<String> = bin_exe
        .iter()
        .map(|(k, v)| format!("{k}={}", v.display()))
        .collect();
    format!("test-{label}-{crate_name}{marker}-{}", exes.join(","))
}

impl TestUnit<'_> {
    fn command(
        &self,
        bin_exe: &[(String, PathBuf)],
        out: &Path,
        script: Option<&Script>,
    ) -> Result<Command> {
        let (sess, pkg, node) = (self.sess, self.pkg, self.node);
        let mut cmd = sess.settings.rustc_cmd(pkg);
        if self.harness {
            cmd.args(["--test", "--emit=dep-info,link", "--cfg", "test"]);
        } else {
            cmd.args([
                "--crate-type",
                "bin",
                "--emit=dep-info,link",
                "--cfg",
                "test",
            ]);
        }
        cmd.envs(bin_exe.iter().map(|(name, path)| (name, path)));
        invoke::set_target_tmpdir(&mut cmd, sess, self.target_tmpdir);
        let mut cmd = invoke::rustc_base(
            cmd,
            sess,
            pkg,
            self.target,
            true,
            &node.features,
            out,
            script,
            true,
        );
        cmd.args(["-C", "panic=unwind"]);
        invoke::add_externs(&mut cmd, sess, node, invoke::ExternSet::Test, false)?;
        invoke::add_natives(&mut cmd, sess, &pkg.id, true);
        if !self.is_lib
            && let Some(art) = sess.get(&pkg.id)
        {
            cmd.arg("--extern")
                .arg(format!("{}={}", art.crate_name, art.path.display()));
        }
        cmd.arg(&self.target.src_path);
        invoke::primary_env(&mut cmd, sess, pkg);
        Ok(cmd)
    }

    fn bin(&self, exe: PathBuf, rustc: RustcOutcome) -> TestBin {
        TestBin {
            exe,
            target: self.target.clone(),
            rustc,
        }
    }

    fn exe(&self) -> executables::Exe<'_> {
        executables::Exe {
            sess: self.sess,
            pkg: self.pkg,
            target: self.target,
        }
    }

    fn compile(
        &self,
        action: &action::Action,
        cmd: &mut Command,
        manifest: &str,
        exe: PathBuf,
    ) -> Result<TestBin> {
        self.exe().build(action, cmd, manifest, &exe)?;
        Ok(self.bin(exe, RustcOutcome::Ran))
    }

    fn restore(
        &self,
        action: &action::Action,
        exe: &Path,
        manifest: &str,
        cmd: &Command,
    ) -> Result<Option<TestBin>> {
        Ok(self
            .exe()
            .restored(action, exe, manifest, cmd)?
            .then(|| self.bin(exe.to_path_buf(), RustcOutcome::Restored)))
    }
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
    let unit = TestUnit {
        sess,
        pkg,
        node,
        target,
        is_lib,
        harness: crate::manifest::harness(&pkg.manifest_path, target),
        target_tmpdir: invoke::uses_target_tmpdir(target),
    };
    let crate_name = target.name.replace('-', "_");
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(|s| s.stamp.clone());
    let keyed = unit_key::unit_digest(
        sess,
        pkg,
        node,
        target,
        &test_kind(label, &crate_name, unit.harness, bin_exe),
        &node.features,
        &[],
        true,
        stamp.as_deref(),
        unit.target_tmpdir,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Test, &keyed.digest)?
        .lineage(keyed.lineage.clone());
    let out = action.out.clone();
    let exe = out.join(executables::exe_file(&crate_name));
    let mut cmd = unit.command(
        bin_exe,
        &action.compile_out(&sess.settings, pkg),
        script.as_ref(),
    )?;
    keyed.tag(&mut cmd);
    let manifest = unit_key::dep_manifest(sess, node, true, (!is_lib).then_some(pkg.id.as_str()))?;
    if let Some(restored) = unit.restore(&action, &exe, &manifest, &cmd)? {
        return Ok(restored);
    }
    unit.compile(&action, &mut cmd, &manifest, exe)
}
