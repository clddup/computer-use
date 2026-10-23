mod cli;
mod platform;
mod runtime;
mod transport;
mod watchdog;
mod worker;
mod worker_backend;
mod worker_query;

use cu_core::DesktopLease;
use cu_protocol::*;
use runtime::Runtime;
use std::{
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    time::Duration,
};
use worker_backend::WorkerBackend;

fn run() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if cli::run_mode(&args)? {
        return Ok(());
    }
    let policy = cli::load_policy(&args)?;
    let cancel = Arc::new(AtomicBool::new(false));
    let signal = cancel.clone();
    ctrlc::set_handler(move || signal.store(true, Ordering::SeqCst))
        .map_err(|e| Error::new("signal_error", e.to_string()))?;
    let rx = transport::read_requests(cancel.clone());
    #[cfg(target_os = "macos")]
    let presentation_cancel = cancel.clone();
    let mut runtime = Runtime::new(
        policy,
        WorkerBackend::new(cancel.clone()),
        cancel,
        DesktopLease::acquire,
    );
    loop {
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => transport::output(&runtime.respond(request)),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
        }
        // Drain queued replies after cancellation; EOF closes the reader's sender.
        runtime.release_if_cancelled();
        #[cfg(target_os = "macos")]
        if let Some(error) = cu_platform_macos::maintain(presentation_cancel.load(Ordering::SeqCst))
        {
            runtime.interrupt(error);
        }
    }
    #[cfg(target_os = "macos")]
    let _ = cu_platform_macos::maintain(true);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
