use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

pub(crate) fn expiry_watchdog(
    cancel: Arc<AtomicBool>,
    lifetime: Duration,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let deadline = Instant::now() + lifetime;
        while !cancel.load(Ordering::SeqCst) {
            if Instant::now() >= deadline {
                cancel.store(true, Ordering::SeqCst);
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expiry_cancels_without_returning_to_dispatch_loop() {
        let cancel = Arc::new(AtomicBool::new(false));
        let worker = expiry_watchdog(cancel.clone(), Duration::from_millis(30));
        worker.join().unwrap();
        assert!(cancel.load(Ordering::SeqCst));
    }
    #[test]
    fn expiry_worker_exits_on_explicit_stop() {
        let cancel = Arc::new(AtomicBool::new(true));
        expiry_watchdog(cancel, Duration::from_secs(3600))
            .join()
            .unwrap();
    }
}
