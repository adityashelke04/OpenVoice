//! Guaranteeing the sidecar dies with the app.
//!
//! The graceful path is that closing the sidecar's stdin makes it exit. That covers
//! a normal shutdown, and it is what `Drop` does.
//!
//! It does not cover a force-kill, a panic that skips unwinding, or a crash — and
//! this was observed for real: killing the parent left a Python process alive
//! holding 1.6 GB of VRAM, which then makes the *next* run fail to allocate. On a
//! 4 GB laptop GPU that is the difference between a working app and a mystery.
//!
//! A Windows job object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` closes that hole
//! at the kernel level: when the last handle to the job goes away — including when
//! the process holding it dies for any reason — every process in the job is
//! terminated. It is the only mechanism that survives `TerminateProcess`.

use std::os::windows::io::AsRawHandle;
use std::process::Child;

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

/// A job object that kills its members when dropped.
pub struct KillOnDrop {
    handle: HANDLE,
}

// The handle is owned exclusively by this struct and only used through the Win32
// calls below, which are thread-safe.
unsafe impl Send for KillOnDrop {}
unsafe impl Sync for KillOnDrop {}

impl KillOnDrop {
    /// Create an empty job configured to terminate its members on close.
    pub fn new() -> Option<Self> {
        // SAFETY: a null name and null security attributes create an unnamed job
        // owned by this process, which is exactly what is wanted.
        let handle = unsafe { CreateJobObjectW(None, None) }.ok()?;

        let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        // SAFETY: `info` is a correctly initialised structure of the size passed,
        // matching the JobObjectExtendedLimitInformation class.
        let ok = unsafe {
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).ok()?,
            )
        };
        if ok.is_err() {
            // SAFETY: `handle` came from a successful CreateJobObjectW.
            unsafe {
                let _ = CloseHandle(handle);
            }
            return None;
        }
        Some(Self { handle })
    }

    /// Put a child process under this job's lifetime.
    pub fn adopt(&self, child: &Child) -> bool {
        let raw = HANDLE(child.as_raw_handle());
        // SAFETY: `raw` is the live process handle owned by `child`, which outlives
        // this call; `self.handle` is a valid job object.
        unsafe { AssignProcessToJobObject(self.handle, raw) }.is_ok()
    }
}

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        // Closing the last handle terminates every process in the job.
        // SAFETY: `handle` came from a successful CreateJobObjectW and is closed
        // exactly once, here.
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}
