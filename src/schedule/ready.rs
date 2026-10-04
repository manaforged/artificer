use super::{Board, Unit};
use std::cell::RefCell;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MetaStage {
    Early,
    Full,
}

#[derive(Clone)]
pub(crate) struct Signal {
    board: Arc<Board>,
    unit: Unit,
    mark: Option<crate::profile::MetaMark>,
}

impl Signal {
    pub(crate) fn metadata_ready(&self, stage: MetaStage) {
        self.board.metadata(&self.unit, stage);
        if let Some(mark) = &self.mark {
            mark.mark();
        }
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Signal>> = const { RefCell::new(None) };
}

pub(crate) fn signal() -> Option<Signal> {
    CURRENT.with_borrow(Clone::clone)
}

pub(super) fn with<R>(board: Arc<Board>, unit: &Unit, work: impl FnOnce() -> R) -> R {
    let signal = Signal {
        board,
        unit: unit.clone(),
        mark: crate::profile::meta_mark(),
    };
    let previous = CURRENT.with_borrow_mut(|slot| slot.replace(signal));
    let result = work();
    CURRENT.with_borrow_mut(|slot| *slot = previous);
    result
}
