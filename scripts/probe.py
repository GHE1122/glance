"""Drives a probe Glance (a debug build: its own settings, mutex, tray and sensors' pipes) for the performance pass
(docs/performance.md): starts it, does a scripted round of things to it, reports its threads, memory and graphics
memory after each, and ends it by its PID, whatever happens.

    python -I scripts/probe.py <glance.exe of a debug build> <script> [rounds]

Scripts:
    settings   open the settings (by starting the exe again, as a second start does) and close them (WM_CLOSE)
    idle       nothing: the probe as it is, sampled every few seconds
"""

import ctypes
import ctypes.wintypes as wt
import subprocess
import sys
import time

sys.path.insert(0, __import__("os").path.dirname(__file__))
import usage  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi")
WM_CLOSE = 0x0010
TH32CS_SNAPTHREAD = 0x4


class THREADENTRY32(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ThreadID", wt.DWORD), ("th32OwnerProcessID", wt.DWORD),
                ("tpBasePri", ctypes.c_long), ("tpDeltaPri", ctypes.c_long), ("dwFlags", wt.DWORD)]


def threads(pid):
    snap = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0)
    entry = THREADENTRY32()
    entry.dwSize = ctypes.sizeof(entry)
    count = 0
    more = kernel32.Thread32First(snap, ctypes.byref(entry))
    while more:
        count += entry.th32OwnerProcessID == pid
        more = kernel32.Thread32Next(snap, ctypes.byref(entry))
    kernel32.CloseHandle(snap)
    return count


def windows_of(pid, class_name):
    found = []

    @ctypes.WINFUNCTYPE(wt.BOOL, wt.HWND, wt.LPARAM)
    def each(hwnd, _):
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        name = ctypes.create_unicode_buffer(64)
        user32.GetClassNameW(hwnd, name, 64)
        if owner.value == pid and name.value == class_name:
            found.append(hwnd)
        return True

    user32.EnumWindows(each, 0)
    return found


def state(pid, query):
    handle = kernel32.OpenProcess(0x1000, False, pid)
    memory = usage.PROCESS_MEMORY_COUNTERS_EX()
    memory.cb = ctypes.sizeof(memory)
    psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb)
    kernel32.CloseHandle(handle)
    usage.pdh.PdhCollectQueryData(query[0])
    dedicated = usage.by_pid(query[1].read(), pid) / 2**20
    return f"threads {threads(pid):4}  private {memory.PrivateUsage / 2**20:7.1f} MB  gpu dedicated {dedicated:7.1f} MB"


def wait_for(condition, seconds):
    until = time.time() + seconds
    while time.time() < until:
        if condition():
            return True
        time.sleep(0.1)
    return False


def main():
    exe, script = sys.argv[1], sys.argv[2]
    rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    query = ctypes.c_void_p()
    usage.pdh.PdhOpenQueryW(None, 0, ctypes.byref(query))
    counters = (query, usage.Counter(query, r"\GPU Process Memory(*)\Dedicated Usage"))
    probe = subprocess.Popen([exe])
    try:
        time.sleep(8)
        print(f"started (pid {probe.pid}): {state(probe.pid, counters)}", flush=True)
        for round_ in range(rounds):
            if script == "settings":
                subprocess.Popen([exe]).wait(timeout=30)
                if not wait_for(lambda: windows_of(probe.pid, "GlanceSettings"), 15):
                    print("  the settings did not open")
                    break
                time.sleep(3)
                opened = state(probe.pid, counters)
                for hwnd in windows_of(probe.pid, "GlanceSettings"):
                    user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
                wait_for(lambda: not windows_of(probe.pid, "GlanceSettings"), 10)
                time.sleep(5)
                print(f"round {round_ + 1}: open {opened}  |  closed {state(probe.pid, counters)}", flush=True)
            else:
                time.sleep(5)
                print(f"after {5 * (round_ + 1)} s: {state(probe.pid, counters)}", flush=True)
    finally:
        probe.kill()
        probe.wait()
        print("probe ended")


if __name__ == "__main__":
    main()
