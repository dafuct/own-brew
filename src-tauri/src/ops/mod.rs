//! Running package operations and reporting them live.
//!
//! Events go back over a per-invocation [`Channel`] rather than a global event
//! bus: two concurrent operations can't cross-talk, and the channel is torn
//! down with the call that created it.

pub mod plan;
pub mod progress;

pub use plan::{Action, Request};

use crate::brew::stream::Origin;
use crate::brew::Brew;
use crate::error::{Error, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Instant;
use tauri::ipc::Channel;
use tokio::sync::oneshot;

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum Event {
    #[serde(rename_all = "camelCase")]
    Started { id: u64, command: String },
    #[serde(rename_all = "camelCase")]
    Phase { id: u64, label: String },
    #[serde(rename_all = "camelCase")]
    Progress { id: u64, percent: f32 },
    #[serde(rename_all = "camelCase")]
    Output {
        id: u64,
        origin: Origin,
        text: String,
    },
    /// Homebrew is blocked on a prompt we cannot answer.
    #[serde(rename_all = "camelCase")]
    NeedsInput { id: u64, text: String },
    #[serde(rename_all = "camelCase")]
    Finished {
        id: u64,
        success: bool,
        cancelled: bool,
        duration_ms: u64,
    },
}

/// How many trailing stderr lines to keep for the failure message.
const ERROR_CONTEXT_LINES: usize = 12;

#[derive(Default)]
pub struct Runner {
    next_id: AtomicU64,
    /// Cancellation handles for operations currently in flight.
    in_flight: Mutex<HashMap<u64, oneshot::Sender<()>>>,
}

impl Runner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Run an operation to completion, streaming progress to `channel`.
    pub async fn run(&self, brew: &Brew, request: Request, channel: Channel<Event>) -> Result<u64> {
        let args = plan::args(&request)?;
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();

        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut stream = brew.stream(&borrowed)?;
        let started = Instant::now();

        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        self.register(id, cancel_tx);

        let _ = channel.send(Event::Started {
            id,
            command: format!("brew {}", args.join(" ")),
        });

        let mut stderr_tail: Vec<String> = Vec::new();
        let mut cancelled = false;

        loop {
            tokio::select! {
                // Cancellation is checked first so a pending cancel wins over
                // a backlog of buffered output.
                biased;

                _ = &mut cancel_rx => {
                    cancelled = true;
                    let _ = stream.kill().await;
                    break;
                }
                line = stream.next_line() => {
                    let Some(line) = line else { break };

                    if line.origin == Origin::Stderr {
                        stderr_tail.push(line.text.clone());
                        if stderr_tail.len() > ERROR_CONTEXT_LINES {
                            stderr_tail.remove(0);
                        }
                    }

                    match progress::interpret(&line.text) {
                        Some(progress::Signal::Phase(label)) => {
                            let _ = channel.send(Event::Phase { id, label });
                        }
                        Some(progress::Signal::Percent(percent)) => {
                            let _ = channel.send(Event::Progress { id, percent });
                        }
                        Some(progress::Signal::NeedsInput) => {
                            let _ = channel.send(Event::NeedsInput { id, text: line.text.clone() });
                        }
                        None => {}
                    }

                    let _ = channel.send(Event::Output {
                        id,
                        origin: line.origin,
                        text: line.text,
                    });
                }
            }
        }

        let status = stream.wait().await?;
        self.forget(id);

        let success = status.success() && !cancelled;
        let _ = channel.send(Event::Finished {
            id,
            success,
            cancelled,
            duration_ms: started.elapsed().as_millis() as u64,
        });

        if cancelled {
            return Err(Error::Cancelled);
        }
        if !success {
            return Err(stream.failure(status, stderr_tail.join("\n")));
        }
        Ok(id)
    }

    /// Ask a running operation to stop.
    pub fn cancel(&self, id: u64) -> Result<()> {
        let sender = self
            .in_flight
            .lock()
            .expect("operation registry poisoned")
            .remove(&id)
            .ok_or(Error::UnknownOperation(id))?;
        // A failed send means the operation finished on its own in the
        // meantime, which is not an error from the caller's point of view.
        let _ = sender.send(());
        Ok(())
    }

    pub fn active(&self) -> Vec<u64> {
        let mut ids: Vec<u64> = self
            .in_flight
            .lock()
            .expect("operation registry poisoned")
            .keys()
            .copied()
            .collect();
        ids.sort_unstable();
        ids
    }

    fn register(&self, id: u64, cancel: oneshot::Sender<()>) {
        self.in_flight
            .lock()
            .expect("operation registry poisoned")
            .insert(id, cancel);
    }

    fn forget(&self, id: u64) {
        self.in_flight
            .lock()
            .expect("operation registry poisoned")
            .remove(&id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancelling_an_unknown_operation_is_an_error() {
        let runner = Runner::new();
        let err = runner.cancel(999).unwrap_err();
        assert_eq!(err.kind(), "unknown_operation");
    }

    #[test]
    fn registered_operations_are_reported_as_active() {
        let runner = Runner::new();
        let (tx, _rx) = oneshot::channel();
        runner.register(7, tx);
        assert_eq!(runner.active(), vec![7]);

        runner.cancel(7).expect("cancel succeeds");
        assert!(runner.active().is_empty(), "cancel deregisters");
    }

    #[test]
    fn ids_are_unique_and_monotonic() {
        let runner = Runner::new();
        let first = runner.next_id.fetch_add(1, Ordering::Relaxed);
        let second = runner.next_id.fetch_add(1, Ordering::Relaxed);
        assert!(second > first);
    }

    #[test]
    fn events_serialize_in_the_shape_the_ui_expects() {
        let json = serde_json::to_value(Event::Progress {
            id: 1,
            percent: 42.5,
        })
        .unwrap();
        assert_eq!(json["event"], "progress");
        assert_eq!(json["data"]["percent"], 42.5);

        let json = serde_json::to_value(Event::Finished {
            id: 1,
            success: false,
            cancelled: true,
            duration_ms: 250,
        })
        .unwrap();
        assert_eq!(json["event"], "finished");
        assert_eq!(json["data"]["durationMs"], 250);
        assert_eq!(json["data"]["cancelled"], true);
    }
}
