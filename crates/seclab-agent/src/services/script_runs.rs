//! Agent 脚本执行器：通过 stdin 安全执行快照，并提供取消、超时和有界输出。

use crate::{models::script_runs, state::AppState, types::ApiResult};
use seclab_contracts::scripts::{
    AgentStartScriptRunRequest, ScriptOutputStream, ScriptRunOutputChunk, ScriptRunStatus,
};
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::Command,
    sync::mpsc,
};

const OUTPUT_LIMIT_BYTES: usize = 256 * 1024;
const TERMINATION_GRACE: Duration = Duration::from_secs(2);

/// 幂等接受一次运行，并在首次创建时转入后台执行。
pub async fn submit(
    state: Arc<AppState>,
    request: AgentStartScriptRunRequest,
) -> ApiResult<String> {
    let (run, created) = script_runs::create(&state.metadata_db, &request).await?;
    if created {
        let run_id = run.run_id.clone();
        tokio::spawn(async move {
            if let Err(error) = execute(state, request).await {
                tracing::error!(%error, %run_id, "script run execution failed unexpectedly");
            }
        });
    }
    Ok(run.run_id)
}

/// 启动时收敛上一次 Agent 进程遗留的活动运行。
pub async fn recover(state: &AppState) -> ApiResult<()> {
    script_runs::recover_interrupted(&state.metadata_db).await
}

async fn execute(state: Arc<AppState>, request: AgentStartScriptRunRequest) -> ApiResult<()> {
    script_runs::mark_starting(&state.metadata_db, &request.run_id).await?;
    if script_runs::cancel_requested(&state.metadata_db, &request.run_id).await? {
        return finish_cancelled(&state, &request.run_id).await;
    }

    let mut command = Command::new("/bin/bash");
    command
        .args(["--noprofile", "--norc", "-s"])
        .env_clear()
        .env(
            "PATH",
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
        )
        .env("HOME", "/tmp")
        .env("LANG", "C.UTF-8")
        .env("LC_ALL", "C.UTF-8")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .process_group(0);
    // SAFETY: 子进程尚未创建，闭包只设置 Linux 父进程死亡信号，不访问共享内存。
    unsafe {
        command.pre_exec(|| {
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return script_runs::finish(
                &state.metadata_db,
                &request.run_id,
                ScriptRunStatus::Failed,
                None,
                Some("SCRIPT_RUN_START_FAILED"),
                Some(&format!("failed to start script process: {error}")),
                &[],
                0,
                false,
            )
            .await;
        }
    };
    let pid = child.id().map(|value| value as libc::pid_t);
    let mut stdin = child.stdin.take().expect("script stdin is piped");
    let source = request.source_content.into_bytes();
    tokio::spawn(async move {
        if stdin.write_all(&source).await.is_ok() {
            let _ = stdin.shutdown().await;
        }
    });

    let (sender, mut receiver) = mpsc::channel::<(ScriptOutputStream, Vec<u8>)>(32);
    let stdout_reader = tokio::spawn(read_stream(
        child.stdout.take(),
        ScriptOutputStream::Stdout,
        sender.clone(),
    ));
    let stderr_reader = tokio::spawn(read_stream(
        child.stderr.take(),
        ScriptOutputStream::Stderr,
        sender,
    ));
    script_runs::mark_running(&state.metadata_db, &request.run_id).await?;

    let deadline =
        tokio::time::Instant::now() + Duration::from_secs(u64::from(request.timeout_seconds));
    let mut poll = tokio::time::interval(Duration::from_millis(200));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut output = OutputCollector::default();
    let outcome = loop {
        tokio::select! {
            status = child.wait() => break RunOutcome::Exited(status),
            chunk = receiver.recv() => {
                if let Some((stream, bytes)) = chunk {
                    output.push(stream, &bytes);
                }
            }
            _ = tokio::time::sleep_until(deadline) => {
                terminate_process_group(pid).await;
                break RunOutcome::TimedOut(child.wait().await);
            }
            _ = poll.tick() => {
                if script_runs::cancel_requested(&state.metadata_db, &request.run_id).await? {
                    terminate_process_group(pid).await;
                    break RunOutcome::Cancelled(child.wait().await);
                }
            }
        }
    };
    while let Some((stream, bytes)) = receiver.recv().await {
        output.push(stream, &bytes);
    }
    let _ = stdout_reader.await;
    let _ = stderr_reader.await;

    let (status, exit_code, error_code, error_summary) = match outcome {
        RunOutcome::Exited(Ok(exit)) if exit.success() => {
            (ScriptRunStatus::Succeeded, exit.code(), None, None)
        }
        RunOutcome::Exited(Ok(exit)) => (
            ScriptRunStatus::Failed,
            exit.code(),
            Some("SCRIPT_RUN_EXIT_FAILED"),
            Some("script exited with a non-zero status"),
        ),
        RunOutcome::Exited(Err(_)) => (
            ScriptRunStatus::Failed,
            None,
            Some("SCRIPT_RUN_WAIT_FAILED"),
            Some("failed to wait for script process"),
        ),
        RunOutcome::TimedOut(result) => (
            ScriptRunStatus::TimedOut,
            result.ok().and_then(|value| value.code()),
            Some("SCRIPT_RUN_TIMED_OUT"),
            Some("script exceeded its timeout"),
        ),
        RunOutcome::Cancelled(result) => (
            ScriptRunStatus::Cancelled,
            result.ok().and_then(|value| value.code()),
            None,
            None,
        ),
    };
    script_runs::finish(
        &state.metadata_db,
        &request.run_id,
        status,
        exit_code,
        error_code,
        error_summary,
        &output.chunks,
        output.size_bytes as u64,
        output.truncated,
    )
    .await
}

async fn finish_cancelled(state: &AppState, run_id: &str) -> ApiResult<()> {
    script_runs::finish(
        &state.metadata_db,
        run_id,
        ScriptRunStatus::Cancelled,
        None,
        None,
        None,
        &[],
        0,
        false,
    )
    .await
}

async fn read_stream<R>(
    reader: Option<R>,
    stream: ScriptOutputStream,
    sender: mpsc::Sender<(ScriptOutputStream, Vec<u8>)>,
) where
    R: AsyncRead + Unpin,
{
    let Some(mut reader) = reader else { return };
    let mut buffer = [0_u8; 8192];
    loop {
        match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read)
                if sender
                    .send((stream, buffer[..read].to_vec()))
                    .await
                    .is_err() =>
            {
                break;
            }
            Ok(_) => {}
        }
    }
}

#[derive(Default)]
struct OutputCollector {
    chunks: Vec<ScriptRunOutputChunk>,
    size_bytes: usize,
    truncated: bool,
}

impl OutputCollector {
    fn push(&mut self, stream: ScriptOutputStream, bytes: &[u8]) {
        let remaining = OUTPUT_LIMIT_BYTES.saturating_sub(self.size_bytes);
        if remaining == 0 {
            self.truncated |= !bytes.is_empty();
            return;
        }
        let mut content = String::from_utf8_lossy(bytes).into_owned();
        if content.len() > remaining {
            let mut boundary = remaining;
            while boundary > 0 && !content.is_char_boundary(boundary) {
                boundary -= 1;
            }
            content.truncate(boundary);
            self.truncated = true;
        }
        if !content.is_empty() {
            self.size_bytes += content.len();
            self.chunks.push(ScriptRunOutputChunk {
                sequence: self.chunks.len() as u64,
                stream,
                content,
            });
        }
        self.truncated |= bytes.len() > remaining;
    }
}

async fn terminate_process_group(pid: Option<libc::pid_t>) {
    let Some(pid) = pid else { return };
    // SAFETY: pid 来自当前运行的子进程，负 pid 只向它的进程组发送信号。
    unsafe {
        libc::kill(-pid, libc::SIGTERM);
    }
    tokio::time::sleep(TERMINATION_GRACE).await;
    // SAFETY: 进程已退出时返回 ESRCH；否则只终止相同进程组。
    unsafe {
        libc::kill(-pid, libc::SIGKILL);
    }
}

enum RunOutcome {
    Exited(std::io::Result<std::process::ExitStatus>),
    TimedOut(std::io::Result<std::process::ExitStatus>),
    Cancelled(std::io::Result<std::process::ExitStatus>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_is_ordered_and_bounded() {
        let mut output = OutputCollector::default();
        output.push(ScriptOutputStream::Stdout, &vec![b'a'; OUTPUT_LIMIT_BYTES]);
        output.push(ScriptOutputStream::Stderr, b"overflow");
        assert_eq!(output.size_bytes, OUTPUT_LIMIT_BYTES);
        assert!(output.truncated);
        assert_eq!(output.chunks[0].sequence, 0);
    }
}
