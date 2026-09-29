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
    for (i, t) in pkg
        .targets
        .iter()
        .filter(|t| t.kind.iter().any(|k| k == "bin"))
        .enumerate()
    {
        if !Package::covered(t, &node.features) {
            continue;
        }
        let name = artifact::bin_name(&t.name);
        if is_bin && i == 0 {
            v.push((name, art.path.clone()));
        } else {
            v.push((name, compile_bin(sess, pkg, node, t)?));
        }
    }
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
    let stamp = script.as_ref().map(Script::stamp);
    let digest = unit_key::unit_digest(
        sess,
        pkg,
        node,
        &format!("{label}-{crate_name}"),
        &node.features,
        &["bin".to_string()],
        true,
        stamp.as_deref(),
        false,
    )?;
    let action = action::Action::begin(&sess.settings.home, action::Kind::Unit, &digest)?;
    let slot = &action.slot;
    let out = action.out.clone();
    let exe = out.join(if cfg!(windows) {
        format!("{crate_name}.exe")
    } else {
        crate_name.clone()
    });

    let mut cmd = sess.settings.rustc_cmd(pkg);
    cmd.args(["--crate-type", "bin", "--emit=dep-info,link"]);
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
    invoke::add_externs(&mut cmd, sess, node, set, false)?;
    invoke::add_natives(&mut cmd, sess);
    if pkg.lib_target().is_some()
        && let Some(lib) = sess.get(&pkg.id)
    {
        cmd.arg("--extern")
            .arg(format!("{}={}", lib.crate_name, lib.path.display()));
    }
    cmd.arg(&target.src_path);
    invoke::primary_env(&mut cmd, sess, pkg);
    let manifest = unit_key::dep_manifest(
        sess,
        node,
        matches!(set, invoke::ExternSet::Test),
        pkg.lib_target().is_some().then_some(pkg.id.as_str()),
    )?;
    if slot.hit() {
        if !unit_key::deps_match(&out, &manifest) || !crate::inputs::matches(&out, pkg.root(), &cmd)
        {
            action.invalidate()?;
        } else if exe.is_file() {
            sess.retain(action.lease()?);
            invoke::replay(sess, pkg, target, &out);
            return Ok(exe);
        }
    }

    action.prepare()?;
    std::fs::create_dir_all(&out)?;
    invoke::note_rustc(&sess.settings.home);
    invoke::run_rustc(&mut cmd, sess, pkg, target, &out)?;
    std::fs::write(out.join(unit_key::DEPS_FILE), &manifest)?;
    if !exe.is_file() {
        bail!("no {label} exe {crate_name} in {}", out.display());
    }
    action.finish()?;
    sess.retain(action.lease()?);
    Ok(exe)
}
