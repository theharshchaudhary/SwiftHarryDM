"""Run a child process inside a Windows Job Object with KILL_ON_JOB_CLOSE.

The child is created suspended, assigned to the job, then resumed, so even
grandchildren it spawns immediately (FFmpeg) belong to the job. Terminating the
job, or this process dying, kills the whole tree: no orphaned FFmpeg processes.
"""
from __future__ import annotations

import ctypes
import os
import subprocess
from ctypes import wintypes as wt

CREATE_SUSPENDED = 0x00000004
CREATE_NEW_PROCESS_GROUP = 0x00000200
CREATE_NO_WINDOW = 0x08000000
JobObjectExtendedLimitInformation = 9
JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE = 0x00002000


class _IO_COUNTERS(ctypes.Structure):
    _fields_ = [(n, ctypes.c_ulonglong) for n in ("ReadOperationCount", "WriteOperationCount",
                                                   "OtherOperationCount", "ReadTransferCount",
                                                   "WriteTransferCount", "OtherTransferCount")]


class _BASIC(ctypes.Structure):
    _fields_ = [("PerProcessUserTimeLimit", ctypes.c_int64), ("PerJobUserTimeLimit", ctypes.c_int64),
                ("LimitFlags", wt.DWORD), ("MinimumWorkingSetSize", ctypes.c_size_t),
                ("MaximumWorkingSetSize", ctypes.c_size_t), ("ActiveProcessLimit", wt.DWORD),
                ("Affinity", ctypes.c_size_t), ("PriorityClass", wt.DWORD), ("SchedulingClass", wt.DWORD)]


class _EXTENDED(ctypes.Structure):
    _fields_ = [("BasicLimitInformation", _BASIC), ("IoInfo", _IO_COUNTERS),
                ("ProcessMemoryLimit", ctypes.c_size_t), ("JobMemoryLimit", ctypes.c_size_t),
                ("PeakProcessMemoryUsed", ctypes.c_size_t), ("PeakJobMemoryUsed", ctypes.c_size_t)]


if os.name == "nt":
    _k32 = ctypes.WinDLL("kernel32", use_last_error=True)
    _k32.CreateJobObjectW.argtypes = [wt.LPVOID, wt.LPCWSTR]
    _k32.CreateJobObjectW.restype = wt.HANDLE
    _k32.SetInformationJobObject.argtypes = [wt.HANDLE, ctypes.c_int, wt.LPVOID, wt.DWORD]
    _k32.SetInformationJobObject.restype = wt.BOOL
    _k32.AssignProcessToJobObject.argtypes = [wt.HANDLE, wt.HANDLE]
    _k32.AssignProcessToJobObject.restype = wt.BOOL
    _k32.TerminateJobObject.argtypes = [wt.HANDLE, wt.UINT]
    _k32.TerminateJobObject.restype = wt.BOOL
    _k32.CloseHandle.argtypes = [wt.HANDLE]
    _k32.CloseHandle.restype = wt.BOOL
    _ntdll = ctypes.WinDLL("ntdll")
    _ntdll.NtResumeProcess.argtypes = [wt.HANDLE]
    _ntdll.NtResumeProcess.restype = ctypes.c_long


class ProcessTree:
    """A started child process plus the means to kill its entire tree."""

    def __init__(self, popen: subprocess.Popen, job_handle):
        self.popen = popen
        self._job = job_handle

    @property
    def pid(self):
        return self.popen.pid

    def kill_tree(self):
        if self._job:
            _k32.TerminateJobObject(self._job, 1)
        else:
            subprocess.run(["taskkill", "/PID", str(self.popen.pid), "/T", "/F"], capture_output=True,
                           creationflags=CREATE_NO_WINDOW)
        try:
            self.popen.wait(5)
        except subprocess.TimeoutExpired:
            self.popen.kill()

    def close(self):
        if self._job:
            _k32.CloseHandle(self._job)   # KILL_ON_JOB_CLOSE: anything left dies here
            self._job = None


def spawn(cmd, **popen_kw) -> ProcessTree:
    if os.name != "nt":   # pragma: no cover - Windows-only product, POSIX fallback for tooling
        return ProcessTree(subprocess.Popen(cmd, start_new_session=True, **popen_kw), None)
    job = _k32.CreateJobObjectW(None, None)
    if job:
        info = _EXTENDED()
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
        if not _k32.SetInformationJobObject(job, JobObjectExtendedLimitInformation, ctypes.byref(info),
                                            ctypes.sizeof(info)):
            _k32.CloseHandle(job)
            job = None
    flags = popen_kw.pop("creationflags", 0) | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    if job:
        flags |= CREATE_SUSPENDED
    p = subprocess.Popen(cmd, creationflags=flags, **popen_kw)
    if job:
        handle = int(p._handle)  # noqa: SLF001 - Popen exposes no public process handle
        if not _k32.AssignProcessToJobObject(job, handle):
            _k32.CloseHandle(job)
            job = None
        _ntdll.NtResumeProcess(handle)
    return ProcessTree(p, job)
