use cu_protocol::*;
use std::{
    io::{self, BufRead, Write},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
    thread,
};

pub(crate) fn output(response: &Response) {
    let bytes = serde_json::to_vec(response).unwrap_or_default();
    let mut stdout = io::stdout().lock();
    if bytes.len() > MAX_FRAME {
        let fallback = Response {
            version: response.version,
            id: response.id.clone(),
            result: None,
            error: Some(Error::new(
                "frame_limit",
                "Response exceeds protocol frame limit",
            )),
        };
        let _ = serde_json::to_writer(&mut stdout, &fallback);
    } else {
        let _ = stdout.write_all(&bytes);
    }
    let _ = stdout.write_all(b"\n");
    let _ = stdout.flush();
}
pub(crate) fn read_frame(reader: &mut impl BufRead) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let data = reader.fill_buf()?;
        if data.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Partial NDJSON frame",
                ))
            };
        }
        let count = data
            .iter()
            .position(|b| *b == b'\n')
            .map(|v| v + 1)
            .unwrap_or(data.len());
        if frame.len() + count > MAX_FRAME {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "NDJSON frame exceeds limit",
            ));
        }
        frame.extend_from_slice(&data[..count]);
        reader.consume(count);
        if frame.last() == Some(&b'\n') {
            return Ok(Some(frame));
        }
    }
}
pub(crate) fn read_requests(cancel: Arc<AtomicBool>) -> mpsc::Receiver<Request> {
    let (tx, rx) = mpsc::sync_channel(32);
    thread::spawn(move || {
        let mut reader = io::stdin().lock();
        loop {
            match read_frame(&mut reader) {
                Ok(Some(frame)) if enqueue(&frame, &tx, &cancel) => {}
                Ok(Some(_)) => break,
                Ok(None) => {
                    cancel.store(true, Ordering::SeqCst);
                    break;
                }
                Err(error) => {
                    cancel.store(true, Ordering::SeqCst);
                    output(&error_response(
                        VERSION,
                        String::new(),
                        "invalid_frame",
                        &error.to_string(),
                    ));
                    break;
                }
            }
        }
    });
    rx
}

fn enqueue(frame: &[u8], tx: &mpsc::SyncSender<Request>, cancel: &AtomicBool) -> bool {
    let request = match serde_json::from_slice::<Request>(frame) {
        Ok(request) => request,
        Err(_) => {
            output(&error_response(
                VERSION,
                String::new(),
                "invalid_params",
                "Invalid protocol request",
            ));
            return true;
        }
    };
    if request.id.is_empty()
        || request.id.len() > 128
        || !(VERSION..=MAX_VERSION).contains(&request.version)
    {
        output(&error_response(
            request.version,
            request.id,
            "protocol_mismatch",
            "Invalid request ID or protocol version",
        ));
        return true;
    }
    if matches!(request.command, Command::Stop { .. }) {
        cancel.store(true, Ordering::SeqCst);
    }
    if let Err(mpsc::TrySendError::Full(request)) = tx.try_send(request) {
        cancel.store(true, Ordering::SeqCst);
        output(&error_response(
            request.version,
            request.id,
            "queue_limit",
            "Request queue full; session stopped",
        ));
        return false;
    }
    true
}

fn error_response(version: u32, id: String, code: &str, message: &str) -> Response {
    Response {
        version,
        id,
        result: None,
        error: Some(Error::new(code, message)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_frames_reject_partial_eof_and_preserve_multiple_lines() {
        assert!(read_frame(&mut io::Cursor::new(b"partial")).is_err());
        let mut input = io::Cursor::new(b"one\ntwo\n");
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"one\n");
        assert_eq!(read_frame(&mut input).unwrap().unwrap(), b"two\n");
        assert!(read_frame(&mut input).unwrap().is_none());
    }
}
