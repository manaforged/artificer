use super::*;

pub(super) fn ship_outputs(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    lib: &cargo::Target,
    out: &Path,
    digest: &str,
    art: &Artifact,
) -> Result<Vec<(String, PathBuf)>> {
    let mut v = Vec::new();
    let is_bin = lib.kind.iter().any(|k| k == "bin");
    if !is_bin {
        for t in artifact::link_types(lib, art.proc_macro) {
            let name = match t.as_str() {
                "cdylib" | "dylib" => artifact::shared_name(&art.crate_name),
                "staticlib" => artifact::static_name(&art.crate_name),
                _ => continue,
            };
            v.push((
                name,
                artifact::find_typed(out, &art.crate_name, digest, &t)?,
            ));
        }
    }
    let mut others = Vec::new();
    for (i, t) in pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"))
        .enumerate()
    {
        if !Package::covered(t, &node.features) {
            continue;
        }
        let first = is_bin && i == 0;
        if !first
            && sess
                .select
                .as_ref()
                .is_some_and(|sel| !sel.wants_bin(&t.name))
        {
            continue;
        }
        let name = artifact::bin_name(&t.name);
        if first {
            v.push((name, art.path.clone()));
        } else {
            others.push((name, t));
        }
    }
    v.extend(crate::schedule::fan_out(others, |(name, t)| {
        Ok((name, compile_bin(sess, pkg, node, t)?))
    })?);
    Ok(v)
}

pub(super) fn compile_bin(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    target: &cargo::Target,
) -> Result<PathBuf> {
    compile_exe(sess, pkg, node, target, "bin", invoke::ExternSet::Lib)
}

pub fn compile_example(
    sess: &Session,
    meta: &cargo::Metadata,
    id: &str,
    name: &str,
) -> Result<PathBuf> {
    let pkg = cargo::package(meta, id)?;
    let node = cargo::node(meta, id)?;
    let target = pkg
        .targets
        .iter()
        .find(|t| t.kind.iter().any(|k| k == "example") && t.name == name)
        .with_context(|| format!("no example target `{name}`"))?;
    if !Package::covered(target, &node.features) {
        bail!("example `{name}` requires features not enabled");
    }
    compile_exe(sess, pkg, node, target, "example", invoke::ExternSet::Test)
}

pub(super) fn exe_file(crate_name: &str) -> String {
    if cfg!(windows) {
        format!("{crate_name}.exe")
    } else {
        crate_name.to_string()
    }
}

pub(super) struct Exe<'u> {
    pub(super) sess: &'u Session,
    pub(super) pkg: &'u Package,
    pub(super) target: &'u cargo::Target,
}

impl Exe<'_> {
    pub(super) fn restored(
        &self,
        action: &action::Action,
        exe: &Path,
        manifest: &str,
        cmd: &Command,
    ) -> Result<bool> {
        let (sess, pkg, out) = (self.sess, self.pkg, &action.out);
        if !action.hit() {
            return Ok(false);
        }
        if !unit_key::deps_match(out, manifest)
            || !crate::inputs::matches(&sess.settings.home, out, pkg.root(), cmd)
        {
            action.invalidate()?;
            return Ok(false);
        }
        if !exe.is_file() {
            return Ok(false);
        }
        sess.retain(action.lease()?);
        invoke::replay(sess, pkg, self.target, out);
        Ok(true)
    }

    pub(super) fn build(
        &self,
        action: &action::Action,
        cmd: &mut Command,
        manifest: &str,
        exe: &Path,
    ) -> Result<()> {
        let (sess, pkg, out) = (self.sess, self.pkg, &action.out);
        action.prepare()?;
        std::fs::create_dir_all(out)?;
        invoke::note_rustc(&sess.settings.home);
        invoke::run_rustc(cmd, sess, pkg, self.target, out, None)?;
        std::fs::write(out.join(unit_key::DEPS_FILE), manifest)?;
        if !exe.is_file() {
            bail!("no executable {} in {}", exe.display(), out.display());
        }
        action.finish()?;
        sess.retain(action.lease()?);
        Ok(())
    }

    fn command(
        &self,
        node: &cargo::Node,
        set: invoke::ExternSet,
        example: bool,
        out: &Path,
        script: Option<&Script>,
    ) -> Result<Command> {
        let (sess, pkg, target) = (self.sess, self.pkg, self.target);
        let mut cmd = sess.settings.rustc_cmd(pkg);
        cmd.args(["--crate-type", "bin", "--emit=dep-info,link"]);
        let mut cmd = invoke::rustc_base(
            cmd,
            sess,
            pkg,
            target,
            true,
            &node.features,
            out,
            script,
            true,
        );
        invoke::add_externs(&mut cmd, sess, node, set, false)?;
        invoke::add_natives(&mut cmd, sess, &pkg.id, example);
        if pkg.lib_target().is_some()
            && let Some(lib) = sess.get(&pkg.id)
        {
            cmd.arg("--extern")
                .arg(format!("{}={}", lib.crate_name, lib.path.display()));
        }
        cmd.arg(&target.src_path);
        invoke::primary_env(&mut cmd, sess, pkg);
        Ok(cmd)
    }
}

fn compile_exe(
    sess: &Session,
    pkg: &Package,
    node: &cargo::Node,
    target: &cargo::Target,
    label: &str,
    set: invoke::ExternSet,
) -> Result<PathBuf> {
    let crate_name = target.name.replace('-', "_");
    let script = ensure_script(sess, pkg, node)?;
    let stamp = script.as_ref().map(|s| s.stamp.clone());
    let keyed = unit_key::unit_digest(
        sess,
        pkg,
        node,
        target,
        &format!("{label}-{crate_name}"),
        &node.features,
        &["bin".to_string()],
        true,
        stamp.as_deref(),
        false,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &keyed.digest)?
        .lineage(keyed.lineage.clone());
    let exe = action.out.join(exe_file(&crate_name));
    let run = Exe { sess, pkg, target };
    let compile_out = action.compile_out(&sess.settings, pkg);
    let mut cmd = run.command(node, set, label == "example", &compile_out, script.as_ref())?;
    keyed.tag(&mut cmd);
    let manifest = unit_key::dep_manifest(
        sess,
        node,
        matches!(set, invoke::ExternSet::Test),
        pkg.lib_target().is_some().then_some(pkg.id.as_str()),
    )?;
    if !run.restored(&action, &exe, &manifest, &cmd)? {
        run.build(&action, &mut cmd, &manifest, &exe)?;
    }
    Ok(exe)
}
