use crate::cargo::{Package, Target, TargetKind};
use crate::key::Scope;

const TARGET_DIRS: [&str; 3] = ["tests", "examples", "benches"];

pub(crate) fn scope(pkg: &Package, own: &Target) -> Scope {
    let root = pkg.root();
    let mut scope = Scope::default();
    if !integration(own) {
        for dir in TARGET_DIRS {
            scope.skip_dir(dir.into());
        }
    }
    for target in &pkg.targets {
        if target.src_path == own.src_path || !(integration(target) || script(target)) {
            continue;
        }
        let Ok(rel) = target.src_path.strip_prefix(root) else {
            continue;
        };
        match rel.parent() {
            Some(dir)
                if rel.file_name() == Some("main.rs".as_ref()) && dir.components().count() > 1 =>
            {
                scope.skip_dir(dir.to_path_buf());
            }
            _ => scope.skip_file(rel.to_path_buf()),
        }
    }
    scope
}

fn integration(target: &Target) -> bool {
    matches!(
        TargetKind::of(target),
        TargetKind::Test | TargetKind::Example | TargetKind::Bench
    )
}

fn script(target: &Target) -> bool {
    TargetKind::of(target) == TargetKind::BuildScript
}
