use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::Value;

/// A short-lived, line-delimited JSON process used only for discovery requests.
pub(crate) struct Server {
    child: Child,
    stdin: ChildStdin,
    receiver: Receiver<std::io::Result<String>>,
    deadline: Instant,
    label: String,
}

impl Server {
    pub(crate) fn spawn(program: &str, args: &[&str], cwd: &Path) -> Result<Self, String> {
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut child = Command::new(program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{program} を起動できません: {e}"))?;
        // Both handles are guaranteed by Stdio::piped().
        let stdout = child.stdout.take().expect("piped stdout");
        let stdin = child.stdin.take().expect("piped stdin");
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            receiver,
            deadline,
            label: program.to_owned(),
        })
    }

    pub(crate) fn send(&mut self, message: Value) -> Result<(), String> {
        writeln!(self.stdin, "{message}")
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("{} に要求を送信できません: {e}", self.label))
    }

    pub(crate) fn receive(&self) -> Result<Value, String> {
        let timeout = || {
            format!(
                "{} のスキル一覧取得が15秒でタイムアウトしました",
                self.label
            )
        };
        let remaining = self
            .deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(timeout)?;
        let line = self
            .receiver
            .recv_timeout(remaining)
            .map_err(|e| match e {
                mpsc::RecvTimeoutError::Timeout => timeout(),
                mpsc::RecvTimeoutError::Disconnected => {
                    format!("{} が応答を返す前に終了しました", self.label)
                }
            })?
            .map_err(|e| format!("{} の応答を読み取れません: {e}", self.label))?;
        serde_json::from_str(&line)
            .map_err(|e| format!("{} の応答が JSON ではありません: {e}", self.label))
    }

    /// Collect a finite command's entire JSON output and require a successful exit.
    pub(crate) fn output(mut self) -> Result<Value, String> {
        let mut output = String::new();
        loop {
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| format!("{} の一覧取得が15秒でタイムアウトしました", self.label))?;
            match self.receiver.recv_timeout(remaining) {
                Ok(line) => {
                    output.push_str(
                        &line.map_err(|e| format!("{} の出力を読み取れません: {e}", self.label))?,
                    );
                    output.push('\n');
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    return Err(format!(
                        "{} の一覧取得が15秒でタイムアウトしました",
                        self.label
                    ));
                }
            }
        }
        loop {
            if let Some(status) = self
                .child
                .try_wait()
                .map_err(|e| format!("{} の終了状態を確認できません: {e}", self.label))?
            {
                if !status.success() {
                    return Err(format!("{} の一覧取得が失敗しました: {status}", self.label));
                }
                break;
            }
            let remaining = self
                .deadline
                .checked_duration_since(Instant::now())
                .ok_or_else(|| format!("{} の一覧取得が15秒でタイムアウトしました", self.label))?;
            std::thread::sleep(remaining.min(Duration::from_millis(10)));
        }
        serde_json::from_str(&output)
            .map_err(|e| format!("{} の出力が JSON ではありません: {e}", self.label))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        // Discovery never starts a model turn, so no work needs to be completed on exit.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn collects_multiline_json_after_successful_exit() {
        let result = Server::spawn(
            "/bin/sh",
            &["-c", "printf '[\\n {\"name\":\"fixture\"}\\n]\\n'"],
            Path::new("/"),
        )
        .unwrap()
        .output()
        .unwrap();
        assert_eq!(result[0]["name"], "fixture");
    }

    #[test]
    fn rejects_successful_json_from_failed_process() {
        let result = Server::spawn("/bin/sh", &["-c", "printf '[]'; exit 7"], Path::new("/"))
            .unwrap()
            .output();
        assert!(result.unwrap_err().contains("失敗"));
    }

    #[test]
    fn stops_process_when_deadline_expires() {
        let mut server =
            Server::spawn("/bin/sh", &["-c", "exec sleep 30"], Path::new("/")).unwrap();
        server.deadline = Instant::now();
        let start = Instant::now();
        assert!(server.output().unwrap_err().contains("タイムアウト"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }
}
