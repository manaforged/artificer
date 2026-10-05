use anyhow::Result;
use std::sync::{Mutex, PoisonError};

pub(crate) fn fan_out<T: Send, R: Send>(
    items: Vec<T>,
    work: impl Fn(T) -> Result<R> + Sync,
) -> Result<Vec<R>> {
    let count = items.len();
    if count <= 1 {
        return items.into_iter().map(work).collect();
    }
    let queue = Mutex::new(items.into_iter().enumerate());
    let results: Mutex<Vec<Option<R>>> = Mutex::new((0..count).map(|_| None).collect());
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let (sink, context) = (crate::out::current(), crate::profile::context());
    std::thread::scope(|scope| {
        for worker in 0..super::job_cap().min(count) {
            let (queue, results, failed, work) = (&queue, &results, &failed, &work);
            let (sink, context) = (sink.clone(), context.clone());
            scope.spawn(move || {
                crate::out::attach(sink);
                crate::profile::enter(&context, worker);
                while failed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .is_none()
                {
                    let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                    let Some((index, item)) = next else {
                        return;
                    };
                    match work(item) {
                        Ok(value) => {
                            results.lock().unwrap_or_else(PoisonError::into_inner)[index] =
                                Some(value);
                        }
                        Err(error) => {
                            failed
                                .lock()
                                .unwrap_or_else(PoisonError::into_inner)
                                .get_or_insert(error);
                        }
                    }
                }
            });
        }
    });
    if let Some(error) = failed.into_inner().unwrap_or_else(PoisonError::into_inner) {
        return Err(error);
    }
    Ok(results
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .into_iter()
        .flatten()
        .collect())
}
