use super::*;
use crate::cargo::TargetKind;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Shape {
    Meta,
    MetaBin,
    Bin,
    Lib,
}

impl Shape {
    fn of(link: bool, target: &cargo::Target) -> Self {
        let bin = TargetKind::of(target) == TargetKind::Bin;
        match (link, bin) {
            (false, false) => Self::Meta,
            (false, true) => Self::MetaBin,
            (true, true) => Self::Bin,
            (true, false) => Self::Lib,
        }
    }

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Meta => "meta",
            Self::MetaBin => "meta-bin",
            Self::Bin => "bin",
            Self::Lib => "lib",
        }
    }

    fn links_output(self) -> bool {
        matches!(self, Self::Bin | Self::Lib)
    }
}

pub(super) struct PackageUnit<'u> {
    sess: &'u Session,
    pkg: &'u Package,
    pub(super) lib: &'u cargo::Target,
    crate_name: String,
    proc_macro: bool,
    pub(super) shape: Shape,
    pub(super) types: Vec<String>,
    pub(super) lto_ok: bool,
}

impl<'u> PackageUnit<'u> {
    pub(super) fn new(sess: &'u Session, pkg: &'u Package) -> Option<Self> {
        let lib = pkg.lib_target().or_else(|| pkg.bin_target())?;
        let proc_macro = pkg.is_proc_macro();
        let shape = Shape::of(sess.needs_link(pkg), lib);
        let types = if shape == Shape::Lib {
            artifact::link_types(lib, proc_macro)
        } else {
            Vec::new()
        };
        let lto_ok = shape == Shape::Bin
            || (!types.is_empty()
                && types
                    .iter()
                    .all(|t| matches!(t.as_str(), "cdylib" | "staticlib" | "dylib")));
        Some(Self {
            sess,
            pkg,
            lib,
            crate_name: lib.name.replace('-', "_"),
            proc_macro,
            shape,
            types,
            lto_ok,
        })
    }

    pub(super) fn pipelines(&self) -> bool {
        matches!(self.shape, Shape::Lib | Shape::Meta)
            && !self.proc_macro
            && self.types.iter().all(|t| t == "rlib")
    }

    fn reads_metadata(&self) -> bool {
        !self.shape.links_output() || self.pipelines()
    }

    pub(super) fn command(
        &self,
        node: &cargo::Node,
        out: &Path,
        stem: &str,
        script: Option<&Script>,
        keyed: &unit_key::UnitKey,
    ) -> Result<Command> {
        let (sess, pkg, lib) = (self.sess, self.pkg, self.lib);
        let link = self.shape.links_output();
        let mut cmd = sess.settings.rustc_cmd(pkg);
        if link {
            cmd.arg(if self.proc_macro {
                "--emit=dep-info,link"
            } else {
                "--emit=dep-info,metadata,link"
            });
            for kind in &self.types {
                cmd.arg("--crate-type").arg(kind);
            }
            if self.shape == Shape::Bin {
                cmd.args(["--crate-type", "bin"]);
            }
            if self.proc_macro {
                cmd.arg("-C").arg("prefer-dynamic");
            }
        } else {
            cmd.arg("--emit=dep-info,metadata");
            cmd.arg("--crate-type")
                .arg(if self.shape == Shape::MetaBin {
                    "bin"
                } else {
                    "lib"
                });
        }
        let mut cmd = invoke::rustc_base(
            cmd,
            sess,
            pkg,
            lib,
            link,
            &node.features,
            out,
            script,
            self.lto_ok,
        );
        cmd.arg("-C").arg(format!("metadata={}", keyed.metadata));
        cmd.arg("-C").arg(format!("extra-filename=-{stem}"));
        invoke::add_externs(
            &mut cmd,
            sess,
            node,
            invoke::ExternSet::Lib,
            self.reads_metadata(),
        )?;
        if self.shape == Shape::Bin || self.proc_macro {
            invoke::add_natives(&mut cmd, sess, &pkg.id, false);
        }
        if self.proc_macro {
            cmd.arg("--extern").arg("proc_macro");
        }
        cmd.arg(&lib.src_path);
        invoke::primary_env(&mut cmd, sess, pkg);
        Ok(cmd)
    }

    pub(super) fn early(&self, out: &Path, digest: &str) -> Option<invoke::Early> {
        self.pipelines().then(|| {
            let stem = format!("lib{}-{digest}", self.crate_name);
            let rmeta = out.join(format!("{stem}.rmeta"));
            let (path, sibling) = match self.shape {
                Shape::Meta => (rmeta.clone(), None),
                _ => (out.join(format!("{stem}.rlib")), Some(rmeta.clone())),
            };
            invoke::Early {
                early_rmeta: out.join(invoke::EARLY_DIR).join(format!("{stem}.rmeta")),
                artifact: Artifact {
                    crate_name: self.crate_name.clone(),
                    path,
                    rmeta: sibling,
                    proc_macro: false,
                },
                rmeta,
            }
        })
    }

    pub(super) fn rustc(
        &self,
        action: &action::Action,
        cmd: &mut Command,
        manifest: &str,
        early: Option<&invoke::Early>,
    ) -> Result<RustcOutcome> {
        let (sess, pkg, out) = (self.sess, self.pkg, &action.out);
        if action.hit() {
            if unit_key::deps_match(out, manifest)
                && crate::inputs::matches(&sess.settings.home, out, pkg.root(), cmd)
            {
                invoke::replay(sess, pkg, self.lib, out);
                return Ok(RustcOutcome::Restored);
            }
            action.invalidate()?;
        }
        action.prepare()?;
        std::fs::create_dir_all(out)?;
        invoke::note_rustc(&sess.settings.home);
        invoke::run_rustc(cmd, sess, pkg, self.lib, out, early)?;
        std::fs::write(out.join(unit_key::DEPS_FILE), manifest)?;
        crate::profile::span(crate::profile::UnitPhase::Publish, || action.finish())?;
        Ok(RustcOutcome::Ran)
    }

    pub(super) fn artifact(&self, out: &Path, digest: &str) -> Result<Artifact> {
        let path = artifact::find_artifact(
            out,
            &self.crate_name,
            self.proc_macro,
            digest,
            self.shape == Shape::Meta,
        )?;
        let rmeta = match path.extension().and_then(|e| e.to_str()) {
            Some("rlib") => {
                let sibling = path.with_extension("rmeta");
                sibling.is_file().then_some(sibling)
            }
            _ => None,
        };
        Ok(Artifact {
            crate_name: self.crate_name.clone(),
            path,
            rmeta,
            proc_macro: self.proc_macro,
        })
    }
}
