"""What Glance's processes take, second by second: processor, memory, graphics memory and engine use, handles,
wakeups. For the performance pass (docs/performance.md, section 3).

    python -I scripts/usage.py <seconds> <out.csv> [process names...]

Process names default to glance.exe, glance-sensors.exe and glance-remote.exe. Each row is one process over one
second. A summary per process (mean, p95, max, and the change from the first to the last minute) is printed at the end.
"""

import csv
import ctypes
import ctypes.wintypes as wt
import statistics
import sys
import time

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
user32 = ctypes.WinDLL("user32", use_last_error=True)
psapi = ctypes.WinDLL("psapi", use_last_error=True)
pdh = ctypes.WinDLL("pdh")

PROCESS_QUERY_LIMITED_INFORMATION = 0x1000
PROCESS_VM_READ = 0x0010
TH32CS_SNAPPROCESS = 0x2
PDH_FMT_DOUBLE = 0x200
PDH_MORE_DATA = 0x800007D2


class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wt.DWORD), ("cntUsage", wt.DWORD), ("th32ProcessID", wt.DWORD), ("th32DefaultHeapID", ctypes.c_void_p),
                ("th32ModuleID", wt.DWORD), ("cntThreads", wt.DWORD), ("th32ParentProcessID", wt.DWORD), ("pcPriClassBase", ctypes.c_long),
                ("dwFlags", wt.DWORD), ("szExeFile", ctypes.c_wchar * 260)]


class PROCESS_MEMORY_COUNTERS_EX(ctypes.Structure):
    _fields_ = [("cb", wt.DWORD), ("PageFaultCount", wt.DWORD), ("PeakWorkingSetSize", ctypes.c_size_t), ("WorkingSetSize", ctypes.c_size_t),
                ("QuotaPeakPagedPoolUsage", ctypes.c_size_t), ("QuotaPagedPoolUsage", ctypes.c_size_t), ("QuotaPeakNonPagedPoolUsage", ctypes.c_size_t),
                ("QuotaNonPagedPoolUsage", ctypes.c_size_t), ("PagefileUsage", ctypes.c_size_t), ("PeakPagefileUsage", ctypes.c_size_t), ("PrivateUsage", ctypes.c_size_t)]


class PDH_FMT_COUNTERVALUE(ctypes.Structure):
    _fields_ = [("CStatus", wt.DWORD), ("doubleValue", ctypes.c_double)]


class PDH_FMT_COUNTERVALUE_ITEM_W(ctypes.Structure):
    _fields_ = [("szName", ctypes.c_wchar_p), ("FmtValue", PDH_FMT_COUNTERVALUE)]


def processes(names):
    """The running processes of those names: pid -> (name, threads)."""
    snap = kernel32.CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0)
    entry = PROCESSENTRY32W()
    entry.dwSize = ctypes.sizeof(entry)
    found = {}
    more = kernel32.Process32FirstW(snap, ctypes.byref(entry))
    while more:
        if entry.szExeFile.lower() in names:
            found[entry.th32ProcessID] = (entry.szExeFile, entry.cntThreads)
        more = kernel32.Process32NextW(snap, ctypes.byref(entry))
    kernel32.CloseHandle(snap)
    return found


class Counter:
    """A PDH counter path with wildcards, read as instance name -> value."""

    def __init__(self, query, path):
        self.handle = ctypes.c_void_p()
        status = pdh.PdhAddEnglishCounterW(query, path, 0, ctypes.byref(self.handle))
        if status:
            raise OSError(f"PdhAddEnglishCounterW({path}) = {status:#x}")

    def read(self):
        size, count = wt.DWORD(0), wt.DWORD(0)
        status = pdh.PdhGetFormattedCounterArrayW(self.handle, PDH_FMT_DOUBLE, ctypes.byref(size), ctypes.byref(count), None)
        if status & 0xFFFFFFFF != PDH_MORE_DATA:
            return {}
        buffer = (ctypes.c_byte * size.value)()
        status = pdh.PdhGetFormattedCounterArrayW(self.handle, PDH_FMT_DOUBLE, ctypes.byref(size), ctypes.byref(count), buffer)
        if status:
            return {}
        items = ctypes.cast(buffer, ctypes.POINTER(PDH_FMT_COUNTERVALUE_ITEM_W))
        return {items[i].szName: items[i].FmtValue.doubleValue for i in range(count.value) if items[i].FmtValue.CStatus in (0, 1)}


def by_pid(values, pid):
    """The sum of a GPU counter's instances of process `pid` (named pid_<pid>_luid_...)."""
    prefix = f"pid_{pid}_"
    return sum(value for name, value in values.items() if name.startswith(prefix))


def main():
    seconds, out = int(sys.argv[1]), sys.argv[2]
    names = {name.lower() for name in (sys.argv[3:] or ["glance.exe", "glance-sensors.exe", "glance-remote.exe"])}
    query = ctypes.c_void_p()
    pdh.PdhOpenQueryW(None, 0, ctypes.byref(query))
    dedicated = Counter(query, r"\GPU Process Memory(*)\Dedicated Usage")
    shared = Counter(query, r"\GPU Process Memory(*)\Shared Usage")
    engine = Counter(query, r"\GPU Engine(*)\Utilization Percentage")
    switches = Counter(query, r"\Thread(*)\Context Switches/sec")
    threads_id = Counter(query, r"\Thread(*)\ID Process")
    pdh.PdhCollectQueryData(query)
    cpus = __import__("os").cpu_count()
    last = {}
    rows = []
    started = time.perf_counter()
    for second in range(seconds + 1):
        time.sleep(max(0.0, started + second - time.perf_counter()))
        pdh.PdhCollectQueryData(query)
        now = time.perf_counter()
        found = processes(names)
        gpu_dedicated, gpu_shared, gpu_engine = dedicated.read(), shared.read(), engine.read()
        owners = threads_id.read()
        switched = switches.read()
        for pid, (name, thread_count) in found.items():
            handle = kernel32.OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, False, pid)
            if not handle:
                continue
            creation, exit_, kernel, user = (wt.FILETIME() for _ in range(4))
            kernel32.GetProcessTimes(handle, ctypes.byref(creation), ctypes.byref(exit_), ctypes.byref(kernel), ctypes.byref(user))
            busy = ((kernel.dwHighDateTime << 32 | kernel.dwLowDateTime) + (user.dwHighDateTime << 32 | user.dwLowDateTime)) / 1e7
            memory = PROCESS_MEMORY_COUNTERS_EX()
            memory.cb = ctypes.sizeof(memory)
            psapi.GetProcessMemoryInfo(handle, ctypes.byref(memory), memory.cb)
            handles = wt.DWORD()
            kernel32.GetProcessHandleCount(handle, ctypes.byref(handles))
            gdi, user_objects = user32.GetGuiResources(handle, 0), user32.GetGuiResources(handle, 1)
            kernel32.CloseHandle(handle)
            before = last.get(pid)
            last[pid] = (busy, now)
            if before is None or second == 0:
                continue
            core = 100.0 * (busy - before[0]) / (now - before[1])
            wakeups = sum(value for instance, value in switched.items() if owners.get(instance) == pid)
            rows.append({
                "second": second, "process": name, "pid": pid,
                "cpu_core_pct": round(core, 3), "cpu_machine_pct": round(core / cpus, 4),
                "private_mb": round(memory.PrivateUsage / 2**20, 2), "working_set_mb": round(memory.WorkingSetSize / 2**20, 2),
                "gpu_dedicated_mb": round(by_pid(gpu_dedicated, pid) / 2**20, 2), "gpu_shared_mb": round(by_pid(gpu_shared, pid) / 2**20, 2),
                "gpu_engine_pct": round(by_pid(gpu_engine, pid), 3), "wakeups_per_s": round(wakeups, 1),
                "handles": handles.value, "gdi": gdi, "user_objects": user_objects, "threads": thread_count,
            })
    with open(out, "w", newline="") as file:
        writer = csv.DictWriter(file, fieldnames=list(rows[0].keys()))
        writer.writeheader()
        writer.writerows(rows)
    summary(rows)


def summary(rows):
    fields = ["cpu_core_pct", "wakeups_per_s", "private_mb", "working_set_mb", "gpu_dedicated_mb", "gpu_shared_mb", "gpu_engine_pct", "handles", "gdi", "user_objects", "threads"]
    for name in sorted({row["process"] for row in rows}):
        mine = [row for row in rows if row["process"] == name]
        print(f"== {name} ({len(mine)} s)")
        for field in fields:
            values = [row[field] for row in mine]
            p95 = sorted(values)[max(0, int(len(values) * 0.95) - 1)]
            first, final = values[: min(60, len(values))], values[-min(60, len(values)):]
            drift = statistics.mean(final) - statistics.mean(first)
            print(f"  {field:18} mean {statistics.mean(values):10.3f}  p95 {p95:10.3f}  max {max(values):10.3f}  first->last minute {drift:+.3f}")


if __name__ == "__main__":
    main()
