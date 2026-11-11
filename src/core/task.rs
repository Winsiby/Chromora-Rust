use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use super::{CoreError, CoreResult};

/// Cooperative cancellation between processing stages and row batches.
#[derive(Clone, Default, Debug)]
pub struct RenderCancellation(Arc<AtomicBool>);

impl RenderCancellation {
    pub fn new() -> Self { Self::default() }
    pub fn cancel(&self) { self.0.store(true, Ordering::Relaxed); }
    pub fn check(&self) -> CoreResult<()> {
        if self.0.load(Ordering::Relaxed) { Err(CoreError::Cancelled) } else { Ok(()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_is_shared_but_new_jobs_are_independent() {
        let token = RenderCancellation::new();
        let worker = token.clone();
        assert!(worker.check().is_ok());
        token.cancel();
        assert!(matches!(worker.check(), Err(CoreError::Cancelled)));
        assert!(RenderCancellation::new().check().is_ok());
    }
}
