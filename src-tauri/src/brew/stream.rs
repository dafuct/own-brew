//! Line-by-line streaming of a running `brew` process.
//!
//! Homebrew interleaves progress on stdout and warnings on stderr, and uses
//! carriage returns to redraw progress in place. We split on both `\n` and `\r`
//! so a redrawn progress line arrives as a series of updates rather than as one
//! enormous line at the very end.

use crate::error::{Error, Result};
use serde::Serialize;
use std::process::ExitStatus;
use tokio::io::{AsyncRead, AsyncReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::mpsc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Serialize)]
pub struct Line {
    pub origin: Origin,
    pub text: String,
}

pub struct Stream {
    child: Child,
    lines: mpsc::UnboundedReceiver<Line>,
    command: String,
}

impl Stream {
    pub fn spawn(mut cmd: Command, command: String) -> Result<Self> {
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("stdout was piped");
        let stderr = child.stderr.take().expect("stderr was piped");

        let (tx, lines) = mpsc::unbounded_channel();
        tokio::spawn(pump(stdout, tx.clone(), Origin::Stdout));
        tokio::spawn(pump(stderr, tx, Origin::Stderr));

        Ok(Self {
            child,
            lines,
            command,
        })
    }

    /// The next line of output, or `None` once both streams have closed.
    pub async fn next_line(&mut self) -> Option<Line> {
        self.lines.recv().await
    }

    /// Stop the process. Homebrew handles SIGKILL mid-install without
    /// corrupting the Cellar: it stages into a temporary keg and only links
    /// once the pour succeeds.
    pub async fn kill(&mut self) -> Result<()> {
        self.child.kill().await?;
        Ok(())
    }

    /// Reap the process and report how it ended.
    pub async fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait().await?)
    }

    /// Turn a non-zero exit into the error the UI will show.
    pub fn failure(&self, status: ExitStatus, stderr: String) -> Error {
        match status.code() {
            Some(code) => Error::BrewFailed {
                command: self.command.clone(),
                code,
                stderr,
            },
            None => Error::BrewTerminated {
                command: self.command.clone(),
            },
        }
    }

    pub fn command(&self) -> &str {
        &self.command
    }
}

async fn pump<R>(reader: R, tx: mpsc::UnboundedSender<Line>, origin: Origin)
where
    R: AsyncRead + Unpin,
{
    let mut reader = BufReader::new(reader);
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];

    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break,
            Ok(read) => {
                pending.extend_from_slice(&chunk[..read]);
                while let Some(at) = pending.iter().position(|b| *b == b'\n' || *b == b'\r') {
                    let mut line: Vec<u8> = pending.drain(..=at).collect();
                    line.pop(); // drop the terminator itself
                    if emit(&tx, origin, &line).is_err() {
                        return; // receiver went away
                    }
                }
            }
            Err(_) => break,
        }
    }

    let _ = emit(&tx, origin, &pending);
}

fn emit(
    tx: &mpsc::UnboundedSender<Line>,
    origin: Origin,
    raw: &[u8],
) -> std::result::Result<(), mpsc::error::SendError<Line>> {
    let text = String::from_utf8_lossy(raw).trim_end().to_owned();
    if text.is_empty() {
        return Ok(());
    }
    tx.send(Line { origin, text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;

    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c")
            .arg(script)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    async fn collect(script: &str) -> Vec<Line> {
        let mut stream = Stream::spawn(sh(script), script.to_owned()).expect("spawn");
        let mut lines = Vec::new();
        while let Some(line) = stream.next_line().await {
            lines.push(line);
        }
        stream.wait().await.expect("wait");
        lines
    }

    #[tokio::test]
    async fn splits_newline_separated_output() {
        let lines = collect("printf 'one\\ntwo\\nthree\\n'").await;
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["one", "two", "three"]);
    }

    #[tokio::test]
    async fn carriage_returns_become_separate_updates() {
        // How a progress bar redraws itself in place.
        let lines = collect("printf '10%%\\r50%%\\r100%%\\n'").await;
        let texts: Vec<_> = lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["10%", "50%", "100%"]);
    }

    #[tokio::test]
    async fn trailing_output_without_a_newline_is_not_lost() {
        let lines = collect("printf 'no trailing newline'").await;
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "no trailing newline");
    }

    #[tokio::test]
    async fn blank_lines_are_dropped() {
        let lines = collect("printf 'a\\n\\n\\nb\\n'").await;
        assert_eq!(lines.len(), 2);
    }

    #[tokio::test]
    async fn stdout_and_stderr_are_distinguished() {
        let lines = collect("echo out; echo err 1>&2").await;
        assert!(lines.iter().any(|l| l.origin == Origin::Stdout && l.text == "out"));
        assert!(lines.iter().any(|l| l.origin == Origin::Stderr && l.text == "err"));
    }

    #[tokio::test]
    async fn exit_code_is_reported() {
        let mut stream = Stream::spawn(sh("exit 42"), "exit 42".to_owned()).expect("spawn");
        while stream.next_line().await.is_some() {}
        let status = stream.wait().await.expect("wait");
        assert_eq!(status.code(), Some(42));
        let err = stream.failure(status, "boom".to_owned());
        assert_eq!(err.kind(), "brew_failed");
    }

    #[tokio::test]
    async fn kill_stops_a_long_running_process() {
        let mut stream = Stream::spawn(sh("sleep 30"), "sleep 30".to_owned()).expect("spawn");
        stream.kill().await.expect("kill");
        let status = stream.wait().await.expect("wait");
        assert!(!status.success());
    }

    #[tokio::test]
    async fn invalid_utf8_does_not_panic() {
        let lines = collect("printf 'good\\n\\xff\\xfe\\n'").await;
        assert!(lines.iter().any(|l| l.text == "good"));
    }
}
