use super::{Side, Sides, dbg_sel};
use crate::cargo::{Metadata, Package};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const PROC_MACRO: &str = " (proc-macro)";
const CRATES_IO: [&str; 2] = [
    "registry+https://github.com/rust-lang/crates.io-index",
    "sparse+https://index.crates.io/",
];
const INDENT_WIDTH: usize = 4;
const INDENT_CHARS: [char; 5] = ['│', '├', '└', '─', ' '];
const BUILD_SECTION: &str = "[build-dependencies]";
const DEV_SECTION: &str = "[dev-dependencies]";

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum TreeSource {
    CratesIo,
    Path(PathBuf),
    Git { url: String, commit: String },
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct TreePkg {
    pub(super) name: String,
    pub(super) version: String,
    pub(super) source: TreeSource,
}

impl TreeSource {
    fn parse(text: &str) -> Self {
        if text.is_empty() {
            return Self::CratesIo;
        }
        if Path::new(text).is_absolute() {
            return Self::Path(PathBuf::from(text));
        }
        match text.rsplit_once('#') {
            Some((url, commit)) if url.contains("://") => Self::Git {
                url: url.to_string(),
                commit: commit.to_string(),
            },
            _ => Self::Other(text.to_string()),
        }
    }

    fn matches(&self, pkg: &Package) -> bool {
        match (self, pkg.source.as_deref()) {
            (Self::Path(dir), None) => pkg.manifest_path.parent() == Some(dir.as_path()),
            (Self::Git { url, commit }, Some(src)) => src
                .strip_prefix("git+")
                .and_then(|rest| rest.rsplit_once('#'))
                .is_some_and(|(locator, full)| {
                    locator == url && !commit.is_empty() && full.starts_with(commit.as_str())
                }),
            (Self::CratesIo, Some(src)) => CRATES_IO.contains(&src),
            _ => false,
        }
    }
}

enum TreeLine<'a> {
    Section {
        depth: usize,
        build: bool,
    },
    Entry {
        depth: usize,
        key: TreePkg,
        proc_macro: bool,
        feats: Vec<&'a str>,
    },
}

fn tree_line(line: &str) -> Option<Option<TreeLine<'_>>> {
    let body = line.trim_start_matches(INDENT_CHARS);
    if body.trim().is_empty() {
        return Some(None);
    }
    let width = line[..line.len() - body.len()].chars().count();
    if !width.is_multiple_of(INDENT_WIDTH) {
        return None;
    }
    let depth = width / INDENT_WIDTH;
    if body.starts_with('[') {
        let build = match body.trim_end() {
            BUILD_SECTION => true,
            DEV_SECTION => false,
            _ => return None,
        };
        return Some(Some(TreeLine::Section { depth, build }));
    }
    let (head, feats) = body.split_once('|')?;
    let feats = feats.trim_end();
    let feats = feats.strip_suffix("(*)").map_or(feats, str::trim_end);
    let head = head.trim();
    let (ident, rest) = head
        .split_once(" (")
        .map_or((head, ""), |(ident, _)| (ident, &head[ident.len()..]));
    let proc_macro = rest.starts_with(PROC_MACRO);
    let rest = rest.strip_prefix(PROC_MACRO).unwrap_or(rest);
    let source = match rest.strip_prefix(" (") {
        Some(inner) => inner.strip_suffix(')')?,
        None if rest.is_empty() => "",
        None => return None,
    };
    let (name, version) = ident.rsplit_once(" v")?;
    let key = TreePkg {
        name: name.trim().to_string(),
        version: version.trim().to_string(),
        source: TreeSource::parse(source),
    };
    Some(Some(TreeLine::Entry {
        depth,
        key,
        proc_macro,
        feats: feats.split(',').filter(|f| !f.is_empty()).collect(),
    }))
}

struct Frame {
    depth: usize,
    host: bool,
    children: bool,
}

pub(super) fn parse_tree(text: &str) -> HashMap<(TreePkg, Side), Vec<String>> {
    walk_tree(text).unwrap_or_default()
}

fn walk_tree(text: &str) -> Option<HashMap<(TreePkg, Side), Vec<String>>> {
    let mut map: HashMap<(TreePkg, Side), Vec<String>> = HashMap::new();
    let mut stack: Vec<Frame> = Vec::new();
    for line in text.lines() {
        match tree_line(line)? {
            None => {}
            Some(TreeLine::Section { depth, build }) => {
                while stack.last().is_some_and(|f| f.depth > depth) {
                    stack.pop();
                }
                let parent = stack.last_mut().filter(|f| f.depth == depth)?;
                parent.children = parent.host || build;
            }
            Some(TreeLine::Entry {
                depth,
                key,
                proc_macro,
                feats,
            }) => {
                while stack.last().is_some_and(|f| f.depth >= depth) {
                    stack.pop();
                }
                let parent = stack.last();
                if parent.map_or(0, |f| f.depth + 1) != depth {
                    return None;
                }
                let host = parent.is_some_and(|f| f.children) || proc_macro;
                stack.push(Frame {
                    depth,
                    host,
                    children: host,
                });
                let side = if host { Side::Host } else { Side::Normal };
                let slot = map.entry((key, side)).or_default();
                for f in feats {
                    if !slot.iter().any(|have| have == f) {
                        slot.push(f.to_string());
                    }
                }
            }
        }
    }
    Some(map)
}

pub(super) fn resolve_ids(
    meta: &Metadata,
    tree: HashMap<(TreePkg, Side), Vec<String>>,
) -> Option<HashMap<String, Sides>> {
    let mut ids: HashMap<String, Sides> = HashMap::new();
    for ((key, side), feats) in tree {
        let same: Vec<&Package> = meta
            .packages
            .iter()
            .filter(|p| p.name == key.name && p.version == key.version)
            .collect();
        let pkg = match same.as_slice() {
            [] => continue,
            [only] => *only,
            _ => {
                let mut hit = same.iter().copied().filter(|p| key.source.matches(p));
                let (Some(one), None) = (hit.next(), hit.next()) else {
                    dbg_sel(&format!("no exact package for {key:?}"));
                    return None;
                };
                one
            }
        };
        let slot = ids.entry(pkg.id.clone()).or_default().get_mut(side);
        if slot.replace(feats).is_some() {
            dbg_sel(&format!("two tree entries map to {} {side:?}", pkg.id));
            return None;
        }
    }
    Some(ids)
}
