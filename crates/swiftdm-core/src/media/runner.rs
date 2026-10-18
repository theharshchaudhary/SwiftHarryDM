//! Run a tool as a cancellable process tree, streaming its output line by line. The process
//! is placed in a Windows Job Object, so stopping it also stops every helper it started
//! (yt-dlp starts FFmpeg), and nothing outlives the app.
use std::collections::VecDeque;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

use crate::cancel::CancelToken;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

struct Job(HANDLE);

// The handle is only used for kernel calls that are safe from any thread.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
    fn new() -> Option<Job> {
        unsafe {
            let h = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                h,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok.is_err() {
                let _ = CloseHandle(h);
                return None;
            }
            Some(Job(h))
        }
    }
    fn assign(&self, child: &tokio::process::Child) {
        if let Some(raw) = child.raw_handle() {
            unsafe {
                let _ = AssignProcessToJobObject(self.0, HANDLE(raw));
            }
        }
    }
    fn kill(&self) {
        unsafe {
            let _ = TerminateJobObject(self.0, 1);
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

#[derive(Debug)]
pub enum Line {
    Out(String),
    Err(String),
}

#[derive(Debug, Default)]
pub struct ProcResult {
    pub code: Option<i32>,
    pub stderr_tail: Vec<String>,
    pub stopped: bool,
    pub timed_out: bool,
}

pub async fn run(cmd: &[String], cancel: &CancelToken, mut on_line: impl FnMut(Line), timeout: Option<Duration>) -> std::io::Result<ProcResult> {
    let (prog, args) = cmd.split_first().ok_or_else(|| std::io::Error::other("empty command"))?;
    let mut c = Command::new(prog);
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("PYTHONIOENCODING", "utf-8")
        .env("PYTHONUTF8", "1")
        .creation_flags(CREATE_NO_WINDOW)
        .kill_on_drop(true);
    let mut child = c.spawn()?;
    let job = Job::new();
    if let Some(j) = &job {
        j.assign(&child);
    }
    let mut out = BufReader::new(child.stdout.take().unwrap()).lines();
    let mut err = BufReader::new(child.stderr.take().unwrap()).lines();
    let mut tail: VecDeque<String> = VecDeque::new();
    let mut res = ProcResult::default();
    let deadline = timeout.map(|t| tokio::time::Instant::now() + t);
    let (mut out_done, mut err_done) = (false, false);
    let kill = |child: &mut tokio::process::Child| {
        if let Some(j) = &job {
            j.kill();
        }
        let _ = child.start_kill();
    };
    while !(out_done && err_done) {
        let sleep = async {
            match deadline {
                Some(d) => tokio::time::sleep_until(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            l = out.next_line(), if !out_done => match l {
                Ok(Some(l)) => on_line(Line::Out(l)),
                _ => out_done = true,
            },
            l = err.next_line(), if !err_done => match l {
                Ok(Some(l)) => {
                    if tail.len() == 200 {
                        tail.pop_front();
                    }
                    tail.push_back(l.clone());
                    on_line(Line::Err(l));
                }
                _ => err_done = true,
            },
            _ = cancel.cancelled() => {
                res.stopped = true;
                kill(&mut child);
                break;
            }
            _ = sleep => {
                res.timed_out = true;
                kill(&mut child);
                break;
            }
        }
    }
    let status = tokio::time::timeout(Duration::from_secs(10), child.wait()).await;
    res.code = match status {
        Ok(Ok(s)) => s.code(),
        _ => {
            kill(&mut child);
            None
        }
    };
    res.stderr_tail = tail.into();
    Ok(res)
}
