use super::Target;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TargetKind {
    Lib,
    Bin,
    Test,
    Example,
    Bench,
    BuildScript,
}

impl TargetKind {
    pub(crate) fn of(target: &Target) -> Self {
        let kinds = target.kind.iter().map(String::as_str);
        kinds
            .filter_map(|kind| match kind {
                "bin" => Some(Self::Bin),
                "test" => Some(Self::Test),
                "example" => Some(Self::Example),
                "bench" => Some(Self::Bench),
                "custom-build" => Some(Self::BuildScript),
                _ => None,
            })
            .next()
            .unwrap_or(Self::Lib)
    }
}
