"""Which of a process's threads take its time and wake it: each thread's name (as Glance names its threads), its
processor time and context switches over a few seconds, the busiest first. For the performance pass
(docs/performance.md): attributes what scripts/usage.py measures as a whole.

    python -I scripts/threads.py [process name] [seconds] [pid]

Defaults: glance.exe, 5 seconds, the first process of that name (a pid picks one of several). Threads with no name are grouped by their start address's module.
"""

import ctypes
import ctypes.wintypes as wt
import sys
import time

kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
ntdll = ctypes.WinDLL("ntdll")
pdh = ctypes.WinDLL("pdh")
psapi = ctypes.WinDLL("psapi")

THREAD_QUERY_LIMITED_INFORMATION = 0x0800
THREAD_QUERY_INFORMATION = 0x0040
PROCESS_QUERY_INFORMATION = 0x0400
PROCESS_VM_READ = 0x0010
PDH_FMT_DOUBLE = 0x200
PDH_FMT_LARGE = 0x400
PDH_MORE_DATA = 0x800007D2


class Value(ctypes.Structure):
    _fields_ = [("CStatus", wt.DWORD), ("value", ctypes.c_double)]


class Item(ctypes.Structure):
    _fields_ = [("szName", ctypes.c_wchar_p), ("FmtValue", Value)]


def counter(query, path):
    handle = ctypes.c_void_p()
    status = pdh.PdhAddEnglishCounterW(query, path, 0, ctypes.byref(handle))
    if status:
        raise OSError(f"{path}: {status:#x}")
    return handle


def read(handle):
    size, count = wt.DWORD(0), wt.DWORD(0)
    if pdh.PdhGetFormattedCounterArrayW(handle, PDH_FMT_DOUBLE, ctypes.byref(size), ctypes.byref(count), None) & 0xFFFFFFFF != PDH_MORE_DATA:
        return {}
    buffer = (ctypes.c_byte * size.value)()
    pdh.PdhGetFormattedCounterArrayW(handle, PDH_FMT_DOUBLE, ctypes.byref(size), ctypes.byref(count), buffer)
    items = ctypes.cast(buffer, ctypes.POINTER(Item))
    return {items[i].szName: items[i].FmtValue.value for i in range(count.value)}


def name_of(tid, modules):
    thread = kernel32.OpenThread(THREAD_QUERY_LIMITED_INFORMATION | THREAD_QUERY_INFORMATION, False, tid)
    if not thread:
        return "?"
    description = ctypes.c_wchar_p()
    named = ""
    if kernel32.GetThreadDescription(thread, ctypes.byref(description)) >= 0 and description.value:
        named = description.value
        kernel32.LocalFree(description)
    start = ctypes.c_void_p()
    ntdll.NtQueryInformationThread(thread, 9, ctypes.byref(start), ctypes.sizeof(start), None)
    kernel32.CloseHandle(thread)
    if named:
        return named
    address = start.value or 0
    for base, end, module in modules:
        if base <= address < end:
            return f"<{module}+{address - base:#x}>"
    return f"<{address:#x}>"


def modules_of(pid):
    process = kernel32.OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, False, pid)
    if not process:
        return []
    handles = (ctypes.c_void_p * 1024)()
    needed = wt.DWORD()
    psapi.EnumProcessModulesEx(process, handles, ctypes.sizeof(handles), ctypes.byref(needed), 3)
    found = []

    class Info(ctypes.Structure):
        _fields_ = [("base", ctypes.c_void_p), ("size", wt.DWORD), ("entry", ctypes.c_void_p)]

    for index in range(needed.value // ctypes.sizeof(ctypes.c_void_p)):
        info = Info()
        psapi.GetModuleInformation(process, ctypes.c_void_p(handles[index]), ctypes.byref(info), ctypes.sizeof(info))
        name = ctypes.create_unicode_buffer(260)
        psapi.GetModuleBaseNameW(process, ctypes.c_void_p(handles[index]), name, 260)
        found.append((info.base or 0, (info.base or 0) + info.size, name.value))
    kernel32.CloseHandle(process)
    return found


def main():
    process = (sys.argv[1] if len(sys.argv) > 1 else "glance.exe").removesuffix(".exe")
    seconds = float(sys.argv[2]) if len(sys.argv) > 2 else 5.0
    wanted = int(sys.argv[3]) if len(sys.argv) > 3 else None
    query = ctypes.c_void_p()
    pdh.PdhOpenQueryW(None, 0, ctypes.byref(query))
    switches = counter(query, rf"\Thread({process}/*)\Context Switches/sec")
    busy = counter(query, rf"\Thread({process}/*)\% Processor Time")
    ids = counter(query, rf"\Thread({process}/*)\ID Thread")
    owner = counter(query, rf"\Thread({process}/*)\ID Process")
    pdh.PdhCollectQueryData(query)
    time.sleep(seconds)
    pdh.PdhCollectQueryData(query)
    switched, used, tids, pids = read(switches), read(busy), read(ids), read(owner)
    # (The instance pattern matches other processes' threads too: only those of the process named, by its pid.)
    pid = wanted or int(next(value for instance, value in pids.items() if instance.split("/")[0].split("#")[0] == process))
    tids = {instance: tid for instance, tid in tids.items() if int(pids.get(instance, -1)) == pid}
    modules = modules_of(pid)
    rows = {}
    for instance, tid in tids.items():
        name = name_of(int(tid), modules)
        group = rows.setdefault(name, [0, 0.0, 0.0])
        group[0] += 1
        group[1] += switched.get(instance, 0.0)
        group[2] += used.get(instance, 0.0)
    print(f"{process} (pid {pid}): {len(tids)} threads, {sum(r[1] for r in rows.values()):.0f} switches/s, {sum(r[2] for r in rows.values()):.1f}% of a core")
    for name, (count, rate, core) in sorted(rows.items(), key=lambda row: -(row[1][1] + 100 * row[1][2])):
        print(f"  {count:4} x {name[:60]:60} {rate:9.1f}/s {core:7.2f}%")


if __name__ == "__main__":
    main()
