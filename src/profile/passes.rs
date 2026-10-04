use super::model::Pass;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::sync::LazyLock;

pub(crate) const FLAGS: [&str; 2] = ["-Ztime-passes", "-Ztime-passes-format=json"];
pub(crate) const BOOTSTRAP: (&str, &str) = ("RUSTC_BOOTSTRAP", "1");
const PREFIX: &[u8] = b"time: ";
const TOTAL: &str = "total";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Parse,
    Expand,
    Resolve,
    Checks,
    Typeck,
    Borrowck,
    Mono,
    Metadata,
    Codegen,
    Llvm,
    Incremental,
    Link,
    Other,
}

impl Category {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Parse => "parse",
            Self::Expand => "expand",
            Self::Resolve => "resolve",
            Self::Checks => "checks",
            Self::Typeck => "typeck",
            Self::Borrowck => "borrowck",
            Self::Mono => "mono",
            Self::Metadata => "metadata",
            Self::Codegen => "codegen",
            Self::Llvm => "llvm",
            Self::Incremental => "incremental",
            Self::Link => "link",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Level {
    Top,
    Inner,
}

const TABLE: [(&str, Category, Level); 18] = [
    ("parse_crate", Category::Parse, Level::Top),
    ("macro_expand_crate", Category::Expand, Level::Top),
    ("resolve_crate", Category::Resolve, Level::Top),
    ("misc_checking_1", Category::Checks, Level::Top),
    ("type_check_crate", Category::Typeck, Level::Top),
    ("MIR_borrow_checking", Category::Borrowck, Level::Top),
    ("misc_checking_3", Category::Checks, Level::Top),
    (
        "monomorphization_collector_root_collections",
        Category::Mono,
        Level::Inner,
    ),
    (
        "monomorphization_collector_graph_walk",
        Category::Mono,
        Level::Inner,
    ),
    (
        "partition_and_assert_distinct_symbols",
        Category::Mono,
        Level::Inner,
    ),
    ("generate_crate_metadata", Category::Metadata, Level::Top),
    ("codegen_crate", Category::Codegen, Level::Top),
    ("LLVM_passes", Category::Llvm, Level::Top),
    (
        "incr_comp_prepare_session_directory",
        Category::Incremental,
        Level::Top,
    ),
    ("serialize_dep_graph", Category::Incremental, Level::Top),
    ("serialize_work_products", Category::Incremental, Level::Top),
    ("link", Category::Link, Level::Top),
    (TOTAL, Category::Other, Level::Top),
];

static LOOKUP: LazyLock<HashMap<&'static str, (Category, Level)>> = LazyLock::new(|| {
    TABLE
        .iter()
        .map(|(name, category, level)| (*name, (*category, *level)))
        .collect()
});

#[derive(Deserialize)]
struct Line {
    pass: String,
    time: f64,
}

fn parse(line: &[u8]) -> Option<Pass> {
    let json = line.strip_prefix(PREFIX)?;
    let parsed: Line = serde_json::from_slice(json).ok()?;
    let us = (parsed.time * 1e6).round();
    Some(Pass {
        name: parsed.pass,
        us: if us.is_finite() && us > 0.0 {
            us as u64
        } else {
            0
        },
    })
}

pub(crate) fn harvest(diagnostics: &Path) -> io::Result<Vec<Pass>> {
    let body = std::fs::read(diagnostics)?;
    let mut kept = Vec::with_capacity(body.len());
    let mut passes = Vec::new();
    let mut found = false;
    for line in body.split_inclusive(|byte| *byte == b'\n') {
        let trimmed = line.strip_suffix(b"\n").unwrap_or(line);
        let trimmed = trimmed.strip_suffix(b"\r").unwrap_or(trimmed);
        if let Some(pass) = parse(trimmed) {
            found = true;
            if LOOKUP.contains_key(pass.name.as_str()) {
                passes.push(pass);
            }
            continue;
        }
        kept.extend_from_slice(line);
    }
    if found {
        crate::platform::replace_atomic(diagnostics, |tmp| std::fs::write(tmp, &kept))?;
    }
    Ok(passes)
}

pub(crate) fn categorize(passes: &[Pass]) -> Vec<(Category, u64)> {
    let mut totals: HashMap<Category, u64> = HashMap::new();
    let mut inner = 0u64;
    let mut total = None;
    for pass in passes {
        let Some((category, level)) = LOOKUP.get(pass.name.as_str()).copied() else {
            continue;
        };
        if pass.name == TOTAL {
            total = Some(pass.us);
            continue;
        }
        let own = match level {
            Level::Inner => {
                inner += pass.us;
                pass.us
            }
            Level::Top => pass.us - std::mem::take(&mut inner).min(pass.us),
        };
        *totals.entry(category).or_default() += own;
    }
    let counted: u64 = totals.values().sum();
    if let Some(total) = total {
        *totals.entry(Category::Other).or_default() += total.saturating_sub(counted);
    }
    let mut out: Vec<(Category, u64)> = totals.into_iter().filter(|(_, us)| *us > 0).collect();
    out.sort();
    out
}

#[cfg(test)]
#[path = "passes_tests.rs"]
mod tests;
