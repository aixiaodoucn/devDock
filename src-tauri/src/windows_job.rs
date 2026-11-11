//! 受管理的 Windows 进程先挂起再纳入 Job，避免子进程脱离管理。
#![cfg(windows)]
use crate::config::Result;
use std::{
    ffi::c_void,
    io, mem,
    os::windows::{io::AsRawHandle, process::CommandExt},
    process::{Child, Command},
    ptr,
};
type Handle = *mut c_void;
#[repr(C)]
#[derive(Default)]
struct BasicLimits {
    process_time: i64,
    job_time: i64,
    flags: u32,
    min_working: usize,
    max_working: usize,
    active: u32,
    affinity: usize,
    priority: u32,
    scheduling: u32,
}
#[repr(C)]
#[derive(Default)]
struct IoCounters {
    read_ops: u64,
    write_ops: u64,
    other_ops: u64,
    read_bytes: u64,
    write_bytes: u64,
    other_bytes: u64,
}
#[repr(C)]
#[derive(Default)]
struct ExtendedLimits {
    basic: BasicLimits,
    io: IoCounters,
    process_memory: usize,
    job_memory: usize,
    peak_process: usize,
    peak_job: usize,
}
#[repr(C)]
struct ThreadEntry {
    size: u32,
    usage: u32,
    id: u32,
    owner: u32,
    priority: i32,
    delta: i32,
    flags: u32,
}
#[link(name = "kernel32")]
unsafe extern "system" {
    fn CreateJobObjectW(attributes: *const c_void, name: *const u16) -> Handle;
    fn SetInformationJobObject(job: Handle, class: i32, info: *const c_void, size: u32) -> i32;
    fn AssignProcessToJobObject(job: Handle, process: Handle) -> i32;
    fn TerminateJobObject(job: Handle, code: u32) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> Handle;
    fn Thread32First(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn Thread32Next(snapshot: Handle, entry: *mut ThreadEntry) -> i32;
    fn OpenThread(access: u32, inherit: i32, id: u32) -> Handle;
    fn ResumeThread(thread: Handle) -> u32;
}
struct OwnedHandle(Handle);
impl Drop for OwnedHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
pub struct Job(OwnedHandle);
// Job 可从取消线程终止，Arc 共享所有权保证句柄在使用期间有效。
unsafe impl Send for Job {}
unsafe impl Sync for Job {}
impl Job {
    pub fn spawn(command: &mut Command) -> Result<(Self, Child)> {
        let raw = unsafe { CreateJobObjectW(ptr::null(), ptr::null()) };
        if raw.is_null() {
            return Err(io::Error::last_os_error().to_string());
        }
        let job = Self(OwnedHandle(raw));
        let mut limits = ExtendedLimits::default();
        limits.basic.flags = 0x2000; // JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if unsafe {
            SetInformationJobObject(
                raw,
                9,
                &limits as *const _ as _,
                mem::size_of_val(&limits) as u32,
            )
        } == 0
        {
            return Err(io::Error::last_os_error().to_string());
        }
        // CREATE_SUSPENDED | CREATE_NO_WINDOW：先分配 Job，输出统一进入工作台日志。
        command.creation_flags(0x4 | 0x08000000);
        let mut child = command.spawn().map_err(|e| e.to_string())?;
        let setup = (|| {
            if unsafe { AssignProcessToJobObject(raw, child.as_raw_handle()) } == 0 {
                return Err(io::Error::last_os_error().to_string());
            }
            resume(child.id())
        })();
        if let Err(error) = setup {
            job.terminate();
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("无法管理 Windows 进程树，命令未执行: {error}"));
        }
        Ok((job, child))
    }
    pub fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0 .0, 1);
        }
    }
}
fn resume(pid: u32) -> Result<()> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(4, 0) }; // TH32CS_SNAPTHREAD
    if snapshot as isize == -1 {
        return Err(io::Error::last_os_error().to_string());
    }
    let snapshot = OwnedHandle(snapshot);
    let mut entry = ThreadEntry {
        size: mem::size_of::<ThreadEntry>() as u32,
        usage: 0,
        id: 0,
        owner: 0,
        priority: 0,
        delta: 0,
        flags: 0,
    };
    let mut found = unsafe { Thread32First(snapshot.0, &mut entry) };
    while found != 0 {
        if entry.owner == pid {
            let thread = unsafe { OpenThread(2, 0, entry.id) }; // THREAD_SUSPEND_RESUME
            if thread.is_null() {
                return Err(io::Error::last_os_error().to_string());
            }
            let thread = OwnedHandle(thread);
            if unsafe { ResumeThread(thread.0) } == u32::MAX {
                return Err(io::Error::last_os_error().to_string());
            }
            return Ok(());
        }
        entry.size = mem::size_of::<ThreadEntry>() as u32;
        found = unsafe { Thread32Next(snapshot.0, &mut entry) };
    }
    Err("找不到待恢复的 Windows 主线程".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        thread,
        time::{Duration, Instant},
    };

    fn wait_for_exit(child: &mut Child, job: &Job) -> std::process::ExitStatus {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                job.terminate();
                let _ = child.kill();
                let _ = child.wait();
                panic!("受管理的 Windows 测试进程未及时退出");
            }
            thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn suspended_process_is_assigned_resumed_and_reaped() {
        let mut command = Command::new("cmd.exe");
        command.args(["/C", "exit", "0"]);
        let (job, mut child) = Job::spawn(&mut command).unwrap();
        assert!(wait_for_exit(&mut child, &job).success());
    }

    #[test]
    fn job_termination_stops_a_managed_process() {
        let mut command = Command::new("powershell.exe");
        command.args(["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]);
        let (job, mut child) = Job::spawn(&mut command).unwrap();
        job.terminate();
        assert!(!wait_for_exit(&mut child, &job).success());
    }
}
