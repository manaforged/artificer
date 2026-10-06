use super::*;
use artificer::{Pick, TargetSel};

#[derive(Clone, Copy)]
enum Flag {
    NoRun,
    Doc,
    NoFailFast,
    Lib,
    Bin,
    Bins,
    Example,
    Examples,
    Test,
    Tests,
    Bench,
    AllTargets,
}

const TEST: &[&str] = &["test"];
const CHECK: &[&str] = &["check"];
const CHECK_OR_BUILD: &[&str] = &["check", "build"];
const LIBRARY: &[&str] = &["check", "build", "test"];
const EXECUTABLE: &[&str] = &["check", "build", "run"];
const PATTERN: [char; 3] = ['*', '?', '['];

const FLAGS: [(&str, Flag, &[&str]); 12] = [
    ("--no-run", Flag::NoRun, TEST),
    ("--doc", Flag::Doc, TEST),
    ("--no-fail-fast", Flag::NoFailFast, TEST),
    ("--lib", Flag::Lib, LIBRARY),
    ("--bin", Flag::Bin, EXECUTABLE),
    ("--bins", Flag::Bins, CHECK_OR_BUILD),
    ("--example", Flag::Example, EXECUTABLE),
    ("--examples", Flag::Examples, CHECK_OR_BUILD),
    ("--test", Flag::Test, LIBRARY),
    ("--tests", Flag::Tests, LIBRARY),
    ("--bench", Flag::Bench, CHECK),
    ("--all-targets", Flag::AllTargets, CHECK_OR_BUILD),
];

impl<'a> Parser<'a> {
    pub(super) fn targets(
        &mut self,
        _arg: &str,
        name: &str,
        attached: Option<&'a str>,
    ) -> Option<Step> {
        let (flag, kind, commands) = FLAGS.iter().find(|(flag, ..)| *flag == name)?;
        if !commands.contains(&self.cmd()) {
            return Some(Err(fallback(format!(
                "{flag} on {} is not modeled",
                self.cmd()
            ))));
        }
        Some(self.target(flag, *kind, attached))
    }

    fn target(&mut self, flag: &str, kind: Flag, attached: Option<&'a str>) -> Step {
        if let Some(step) = self.whole(kind) {
            return step;
        }
        let name = self.value(flag, attached)?;
        if name.contains(PATTERN) {
            return Err(fallback(format!("{flag} name patterns are not modeled")));
        }
        let sel = &mut self.out.select;
        let pick = match kind {
            Flag::Bin => &mut sel.bins,
            Flag::Example => &mut sel.examples,
            Flag::Test => &mut sel.tests,
            _ => &mut sel.benches,
        };
        pick.add(name);
        Ok(Flow::Next)
    }

    fn whole(&mut self, kind: Flag) -> Option<Step> {
        let sel = &mut self.out.select;
        let step = match kind {
            Flag::NoRun => set(&mut self.out.no_run),
            Flag::Doc => set(&mut self.out.doc_only),
            Flag::NoFailFast => Ok(Flow::Next),
            Flag::Lib => set(&mut sel.lib),
            Flag::AllTargets => {
                *sel = TargetSel::all_targets();
                Ok(Flow::Next)
            }
            Flag::Bins => all(&mut sel.bins),
            Flag::Examples => all(&mut sel.examples),
            Flag::Tests => all(&mut sel.tests),
            Flag::Bin | Flag::Example | Flag::Test | Flag::Bench => return None,
        };
        Some(step)
    }

    pub(super) fn selection_reason(&self) -> Option<&'static str> {
        let sel = &self.out.select;
        match self.cmd() {
            "run" => {
                let one = |pick: &Pick| matches!(pick, Pick::Named(names) if names.len() == 1);
                let only_bin = TargetSel {
                    bins: sel.bins.clone(),
                    ..TargetSel::default()
                };
                let only_example = TargetSel {
                    examples: sel.examples.clone(),
                    ..TargetSel::default()
                };
                let fits = sel.is_default()
                    || (one(&sel.bins) && *sel == only_bin)
                    || (one(&sel.examples) && *sel == only_example);
                (!fits).then_some("run target selection belongs to cargo")
            }
            "test" if sel.lib && !sel.tests.is_none() => {
                Some("test --lib with --test is not modeled")
            }
            _ => None,
        }
    }
}

fn all(pick: &mut Pick) -> Step {
    *pick = Pick::All;
    Ok(Flow::Next)
}
