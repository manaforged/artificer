use artificer::ColorChoice;
use std::path::{Path, PathBuf};

#[derive(Default)]
pub struct BuildArgs {
    pub cmd: String,
    pub quiet: bool,
    pub verbose: bool,
    pub color: Option<ColorChoice>,
    pub jobs: Option<usize>,
    pub packages: Vec<String>,
    pub dir: Option<PathBuf>,
    pub target_dir: Option<PathBuf>,
    pub no_run: bool,
    pub json: bool,
    pub all_features: bool,
    pub features: Vec<String>,
    pub no_default: bool,
    pub meta_flags: Vec<String>,
    pub release: bool,
    pub workspace: bool,
    pub doc_only: bool,
    pub select: artificer::TargetSel,
    pub pass: Vec<String>,
    pub timings: bool,
}

pub enum Parsed {
    Build(Box<BuildArgs>),
    Fallback(String),
    Unknown,
}

type Step = Result<Flow, Parsed>;

enum Flow {
    Next,
    Stop,
}

#[derive(Default)]
struct Seen {
    manifest: bool,
    profile: bool,
    release: bool,
}

struct Parser<'a> {
    args: &'a [String],
    i: usize,
    shim: bool,
    seen: Seen,
    out: BuildArgs,
}

pub fn parse(argv: &[String], shim: bool) -> Parsed {
    let (globals, at) = match globals(argv) {
        Ok(found) => found,
        Err(parsed) => return parsed,
    };
    let Some(cmd) = argv.get(at) else {
        return Parsed::Unknown;
    };
    let cmd = cmd.as_str();
    if !COMMANDS.contains(&cmd) || (shim && DIRECT_ONLY.contains(&cmd)) {
        return Parsed::Unknown;
    }
    let mut parser = Parser {
        args: &argv[at + 1..],
        i: 0,
        shim,
        seen: Seen::default(),
        out: BuildArgs {
            cmd: cmd.to_string(),
            ..globals
        },
    };
    while parser.i < parser.args.len() {
        match parser.step() {
            Ok(Flow::Next) => parser.i += 1,
            Ok(Flow::Stop) => break,
            Err(parsed) => return parsed,
        }
    }
    parser.finish()
}

fn globals(argv: &[String]) -> Result<(BuildArgs, usize), Parsed> {
    let mut out = BuildArgs::default();
    let mut at = 0;
    while let Some(arg) = argv.get(at) {
        match arg.as_str() {
            "--color" => {
                out.color = Some(color_choice(argv.get(at + 1).map(String::as_str))?);
                at += 1;
            }
            flag if flag.starts_with("--color=") => {
                out.color = Some(color_choice(attached(flag, "--color="))?);
            }
            "-q" | "--quiet" => out.quiet = true,
            "-v" | "--verbose" => out.verbose = true,
            _ => break,
        }
        at += 1;
    }
    Ok((out, at))
}

fn split(arg: &str) -> (&str, Option<&str>) {
    match arg.split_once('=') {
        Some((name, value)) if VALUED.contains(&name) => (name, Some(value)),
        _ => (arg, None),
    }
}

impl<'a> Parser<'a> {
    fn cmd(&self) -> &str {
        &self.out.cmd
    }

    fn value(&mut self, name: &str, attached: Option<&'a str>) -> Result<&'a str, Parsed> {
        match attached {
            Some(value) if !value.is_empty() => Ok(value),
            Some(_) => Err(fallback(format!("{name} needs a value"))),
            None => value(self.args, &mut self.i)
                .ok_or_else(|| fallback(format!("{name} needs a value"))),
        }
    }

    fn step(&mut self) -> Step {
        let arg = self.args[self.i].as_str();
        if arg == "--" {
            self.out
                .pass
                .extend(self.args[self.i + 1..].iter().cloned());
            return Ok(Flow::Stop);
        }
        let (name, attached) = split(arg);
        let groups = [
            Self::scope,
            Self::targets,
            Self::profile,
            Self::features,
            Self::output,
        ];
        for group in groups {
            if let Some(step) = group(self, arg, name, attached) {
                return step;
            }
        }
        if arg.starts_with('-') {
            return Err(fallback(format!("{arg} belongs to cargo")));
        }
        self.positional(arg)
    }

    fn scope(&mut self, _arg: &str, name: &str, attached: Option<&'a str>) -> Option<Step> {
        let step = match name {
            "-p" | "--package" => self.value(name, attached).and_then(|package| {
                if package.contains('@') {
                    return Err(fallback(
                        "package specifications with versions belong to cargo",
                    ));
                }
                self.out.packages.push(package.to_string());
                Ok(Flow::Next)
            }),
            "--manifest-path" => self.value(name, attached).and_then(|path| {
                if self.seen.manifest {
                    return Err(fallback("multiple manifest paths belong to cargo"));
                }
                let parent = manifest_dir(path)
                    .ok_or_else(|| fallback("this manifest path belongs to cargo"))?;
                self.seen.manifest = true;
                self.out.dir = Some(parent);
                Ok(Flow::Next)
            }),
            "--target-dir" => self.value(name, attached).and_then(|path| {
                if self.out.target_dir.is_some() {
                    return Err(fallback("multiple target directories belong to cargo"));
                }
                let dir = std::path::absolute(path).map_err(|e| {
                    fallback(format!("target directory {path} belongs to cargo: {e}"))
                })?;
                self.out.target_dir = Some(dir);
                Ok(Flow::Next)
            }),
            "--workspace" if self.cmd() != "run" => {
                self.out.workspace = true;
                Ok(Flow::Next)
            }
            "--workspace" => Err(fallback("run --workspace belongs to cargo")),
            _ => return None,
        };
        Some(step)
    }

    fn profile(&mut self, _arg: &str, name: &str, attached: Option<&'a str>) -> Option<Step> {
        let step = match name {
            "--release" if self.seen.profile => {
                Err(fallback("--release with --profile belongs to cargo"))
            }
            "--release" => {
                self.seen.release = true;
                set(&mut self.out.release)
            }
            "--profile" => self.value(name, attached).and_then(|profile| {
                if self.seen.release || self.seen.profile {
                    return Err(fallback("multiple profile selectors belong to cargo"));
                }
                self.seen.profile = true;
                self.out.release = match profile {
                    "release" => true,
                    "dev" => false,
                    _ => return Err(fallback(format!("profile `{profile}` belongs to cargo"))),
                };
                Ok(Flow::Next)
            }),
            "-j" | "--jobs" => {
                let problem = if attached.is_some() {
                    "--jobs needs a number"
                } else {
                    "an explicit job count needs a number"
                };
                self.value(name, attached)
                    .and_then(|count| self.jobs(count, problem))
            }
            flag if flag.starts_with("-j") && flag.len() > 2 && !flag.starts_with("--") => {
                self.jobs(&flag[2..], "an explicit job count needs a number")
            }
            _ => return None,
        };
        Some(step)
    }

    fn jobs(&mut self, count: &str, problem: &str) -> Step {
        let count = count
            .parse::<usize>()
            .ok()
            .ok_or_else(|| fallback(problem))?;
        self.out.jobs = Some(count.max(1));
        Ok(Flow::Next)
    }

    fn features(&mut self, arg: &str, name: &str, attached: Option<&'a str>) -> Option<Step> {
        let step = match name {
            "--all-features" => set(&mut self.out.all_features),
            "--no-default-features" => set(&mut self.out.no_default),
            "--locked" | "--offline" | "--frozen" => {
                self.out.meta_flags.push(arg.to_string());
                Ok(Flow::Next)
            }
            "--features" | "-F" => self.value(name, attached).and_then(|list| {
                if list.split([',', ' ']).any(str::is_empty) {
                    return Err(fallback("--features has an empty feature name"));
                }
                self.out
                    .features
                    .extend(list.split([',', ' ']).map(str::to_string));
                Ok(Flow::Next)
            }),
            _ => return None,
        };
        Some(step)
    }

    fn output(&mut self, _arg: &str, name: &str, attached: Option<&'a str>) -> Option<Step> {
        let step = match name {
            "--message-format" => self.value(name, attached).and_then(|format| {
                match format {
                    "human" => {}
                    "json" | "json-render-diagnostics" if self.cmd() != "run" => {
                        self.out.json = true;
                    }
                    _ => {
                        return Err(fallback(format!(
                            "message format `{format}` is not modeled"
                        )));
                    }
                }
                Ok(Flow::Next)
            }),
            "--quiet" | "-q" => set(&mut self.out.quiet),
            "--verbose" | "-v" => set(&mut self.out.verbose),
            "--keep-going" => Err(fallback("--keep-going belongs to cargo")),
            "--timings" => match attached {
                None | Some("html") => set(&mut self.out.timings),
                Some(other) => Err(fallback(format!("--timings={other} belongs to cargo"))),
            },
            "--color" => self
                .value(name, attached)
                .and_then(|value| color_choice(Some(value)))
                .map(|choice| {
                    self.out.color = Some(choice);
                    Flow::Next
                }),
            _ => return None,
        };
        Some(step)
    }

    fn positional(&mut self, arg: &str) -> Step {
        let (shim, cmd) = (self.shim, self.out.cmd.as_str());
        match cmd {
            "run" if shim => {
                self.out.pass.extend(self.args[self.i..].iter().cloned());
                return Ok(Flow::Stop);
            }
            "test" if shim && !self.out.pass.is_empty() => {
                return Err(fallback("multiple test filters belong to cargo"));
            }
            "test" if shim => self.out.pass.push(arg.to_string()),
            _ if shim => return Err(fallback("positional arguments belong to cargo")),
            "test" if !PathBuf::from(arg).is_dir() => self.out.pass.push(arg.to_string()),
            _ => self.out.dir = Some(PathBuf::from(arg)),
        }
        Ok(Flow::Next)
    }

    fn finish(self) -> Parsed {
        if let Some(reason) = self.selection_reason() {
            return fallback(reason);
        }
        Parsed::Build(Box::new(self.out))
    }
}

fn set(flag: &mut bool) -> Step {
    *flag = true;
    Ok(Flow::Next)
}

fn fallback(reason: impl Into<String>) -> Parsed {
    Parsed::Fallback(reason.into())
}

mod targets;
mod values;
use values::{COMMANDS, DIRECT_ONLY, VALUED, attached, color_choice, manifest_dir, value};
