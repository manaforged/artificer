use std::process::ExitCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Sub {
    Check,
    Build,
    Run,
    Test,
    Warm,
    Clean,
    Serve,
    Stat,
    Doctor,
    WhyMiss,
    WhyFallback,
    Env,
    Enable,
    Disable,
    Install,
    Uninstall,
    Export,
    Import,
    Mods,
    Remote,
    Pull,
}

type Opt = (&'static str, &'static str);

struct Entry {
    sub: Sub,
    name: &'static str,
    summary: &'static str,
    usage: &'static str,
    options: &'static [&'static [Opt]],
}

const PACKAGE: &[Opt] = &[
    ("-p, --package <SPEC>", "Package to build (repeatable)"),
    ("--manifest-path <PATH>", "Path to Cargo.toml"),
    ("-F, --features <FEATURES>", "Features to activate"),
    ("--all-features", "Activate all features"),
    (
        "--no-default-features",
        "Do not activate the `default` feature",
    ),
    ("--release", "Build with the release profile"),
    ("--profile <NAME>", "Build with the named profile"),
    ("--locked, --offline, --frozen", "Passed to cargo metadata"),
    ("-j, --jobs <N>", "Cap compile workers"),
];
const WORKSPACE: &[Opt] = &[("--workspace", "Build every workspace member")];
const TARGETS: &[Opt] = &[
    ("--tests", "Also compile test targets"),
    ("--all-targets", "Compile every target"),
    ("--message-format <FMT>", "Diagnostic format"),
];
const TEST: &[Opt] = &[
    ("--no-run", "Compile, but do not run the tests"),
    ("--lib", "Test only the library"),
    ("--doc", "Run only doctests"),
    ("--test <NAME>", "Run only the named integration test"),
    ("--message-format <FMT>", "Diagnostic format"),
    ("-- <ARGS>", "Arguments for the test harness"),
];
const RUN: &[Opt] = &[
    ("--bin <NAME>", "Binary to run"),
    ("--example <NAME>", "Example to run"),
    ("-- <ARGS>", "Arguments for the program"),
];
const GLOBAL: &[Opt] = &[
    ("-q, --quiet", "Silence Artificer progress lines"),
    ("-v, --verbose", "Print each rustc command"),
    ("--color <WHEN>", "auto, always, or never"),
    ("-h, --help", "Print help"),
];
const HELP_ONLY: &[Opt] = &[("-h, --help", "Print help")];

const ENTRIES: [Entry; 21] = [
    Entry {
        sub: Sub::Check,
        name: "check",
        summary: "Check a package or workspace",
        usage: "artificer check [OPTIONS] [DIR]",
        options: &[PACKAGE, WORKSPACE, TARGETS, GLOBAL],
    },
    Entry {
        sub: Sub::Build,
        name: "build",
        summary: "Build a package or workspace",
        usage: "artificer build [OPTIONS] [DIR]",
        options: &[PACKAGE, WORKSPACE, TARGETS, GLOBAL],
    },
    Entry {
        sub: Sub::Run,
        name: "run",
        summary: "Build and run a binary or example",
        usage: "artificer run [OPTIONS] [DIR] [-- ARGS]",
        options: &[PACKAGE, RUN, GLOBAL],
    },
    Entry {
        sub: Sub::Test,
        name: "test",
        summary: "Build and run tests",
        usage: "artificer test [OPTIONS] [DIR] [-- ARGS]",
        options: &[PACKAGE, WORKSPACE, TEST, GLOBAL],
    },
    Entry {
        sub: Sub::Warm,
        name: "warm",
        summary: "Populate the store for a workspace",
        usage: "artificer warm [OPTIONS] [DIR]",
        options: &[PACKAGE, GLOBAL],
    },
    Entry {
        sub: Sub::Clean,
        name: "clean",
        summary: "Evict unused units and drop Cargo target caches",
        usage: "artificer clean [DIR]",
        options: &[GLOBAL],
    },
    Entry {
        sub: Sub::Serve,
        name: "serve",
        summary: "Run the local compilation daemon",
        usage: "artificer serve [stop]",
        options: &[&[("stop", "Stop the running daemon")], HELP_ONLY],
    },
    Entry {
        sub: Sub::Stat,
        name: "stat",
        summary: "Show store and mod status",
        usage: "artificer stat [--json]",
        options: &[&[("--json", "Print one JSON object")], HELP_ONLY],
    },
    Entry {
        sub: Sub::Doctor,
        name: "doctor",
        summary: "Check the shim, Cargo fallback, store, and daemon",
        usage: "artificer doctor",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::WhyMiss,
        name: "why-miss",
        summary: "Show what changed between the last two key records of a crate",
        usage: "artificer why-miss CRATE",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::WhyFallback,
        name: "why-fallback",
        summary: "Show why handled commands fell back to stock Cargo",
        usage: "artificer why-fallback [--limit N]",
        options: &[&[("--limit <N>", "Reasons to show (default 5)")], HELP_ONLY],
    },
    Entry {
        sub: Sub::Env,
        name: "env",
        summary: "Print a command that adds the Cargo shim to PATH",
        usage: "artificer env",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::Enable,
        name: "enable",
        summary: "Enable caching for this store",
        usage: "artificer enable",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::Disable,
        name: "disable",
        summary: "Send shim commands directly to Cargo for this store",
        usage: "artificer disable",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::Install,
        name: "install",
        summary: "Install the cargo shim and put it first on PATH",
        usage: "artificer install [--no-modify-path] [--remote LOCATION]",
        options: &[
            &[
                (
                    "--no-modify-path",
                    "Print the PATH line instead of editing profiles",
                ),
                (
                    "--remote <LOCATION>",
                    "Set the remote store and start the first pull",
                ),
            ],
            HELP_ONLY,
        ],
    },
    Entry {
        sub: Sub::Uninstall,
        name: "uninstall",
        summary: "Remove the shim and its PATH lines",
        usage: "artificer uninstall [--purge]",
        options: &[&[("--purge", "Also delete the cache")], HELP_ONLY],
    },
    Entry {
        sub: Sub::Export,
        name: "export",
        summary: "Copy recently used units to a directory for a CI cache",
        usage: "artificer export DIR [--days N] [--max-gb N]",
        options: &[
            &[
                ("--days <N>", "Units used in the last N days (default 7)"),
                ("--max-gb <N>", "Size cap in GB (default 2)"),
            ],
            HELP_ONLY,
        ],
    },
    Entry {
        sub: Sub::Import,
        name: "import",
        summary: "Add missing units from an exported directory",
        usage: "artificer import DIR",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::Mods,
        name: "mods",
        summary: "List or change optional compile modes",
        usage: "artificer mods [on|off NAME]",
        options: &[HELP_ONLY],
    },
    Entry {
        sub: Sub::Remote,
        name: "remote",
        summary: "Show, set, or clear the remote store that pull reads",
        usage: "artificer remote [set LOCATION | off]",
        options: &[
            &[
                (
                    "set <LOCATION>",
                    "HOST:/ABSOLUTE/PATH over ssh, or an absolute directory",
                ),
                ("off", "Clear the remote"),
            ],
            HELP_ONLY,
        ],
    },
    Entry {
        sub: Sub::Pull,
        name: "pull",
        summary: "Add missing units from the remote store",
        usage: "artificer pull",
        options: &[HELP_ONLY],
    },
];

const HEADER: &str = "Artificer is a compile cache that every Rust checkout on this machine shares.

Usage: artificer <COMMAND> [OPTIONS] [PATH]
       artificer help <COMMAND>";

const FOOTER: &str = "Commands and flags the shim does not support run under real Cargo.
Run `artificer doctor` to check an installation.";

pub(super) fn lookup(name: &str) -> Option<Sub> {
    ENTRIES.iter().find(|e| e.name == name).map(|e| e.sub)
}

pub(super) fn overview() -> String {
    let width = ENTRIES.iter().map(|e| e.name.len()).max().unwrap_or(0);
    let rows: Vec<String> = ENTRIES
        .iter()
        .map(|e| format!("  {:width$}  {}", e.name, e.summary))
        .collect();
    format!("{HEADER}\n\nCommands:\n{}\n\n{FOOTER}", rows.join("\n"))
}

pub(super) fn page(sub: Sub) -> String {
    let entry = ENTRIES
        .iter()
        .find(|e| e.sub == sub)
        .expect("every subcommand has a help entry");
    let opts: Vec<Opt> = entry
        .options
        .iter()
        .flat_map(|s| s.iter().copied())
        .collect();
    let width = opts.iter().map(|(flag, _)| flag.len()).max().unwrap_or(0);
    let rows: Vec<String> = opts
        .iter()
        .map(|(flag, text)| format!("  {flag:width$}  {text}"))
        .collect();
    format!(
        "{}\n\nUsage: {}\n\nOptions:\n{}",
        entry.summary,
        entry.usage,
        rows.join("\n")
    )
}

pub(super) fn usage(sub: Sub) -> ExitCode {
    let entry = ENTRIES.iter().find(|e| e.sub == sub);
    if let Some(entry) = entry {
        eprintln!("usage: {}", entry.usage);
    }
    ExitCode::from(2)
}

pub(super) fn wants_help(args: &[String]) -> bool {
    args.iter()
        .take_while(|arg| *arg != "--")
        .any(|arg| arg == "-h" || arg == "--help")
}

pub(super) fn help_cmd(args: &[String]) -> ExitCode {
    match args {
        [] => {
            println!("{}", overview());
            ExitCode::SUCCESS
        }
        [name] => match lookup(name) {
            Some(sub) => {
                println!("{}", page(sub));
                ExitCode::SUCCESS
            }
            None => {
                eprintln!("artificer: no command `{name}`");
                ExitCode::from(2)
            }
        },
        _ => {
            eprintln!("usage: artificer help [COMMAND]");
            ExitCode::from(2)
        }
    }
}

pub(super) fn subcommand(args: &[String]) -> Option<(Sub, usize)> {
    let mut at = 0;
    while let Some(arg) = args.get(at) {
        match arg.as_str() {
            "--color" => at += 2,
            flag if flag.starts_with('-') => at += 1,
            name => return lookup(name).map(|sub| (sub, at)),
        }
    }
    None
}
