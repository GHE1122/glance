"""Drives a probe Glance (a debug build: its own settings, mutex, tray and sensors' pipes) for the performance pass
(docs/performance.md): starts it with its marks written (see src/perf.rs), does a scripted round of things to it the
way a user's input would, and reports how long each took, from its marks; ends it by its PID, whatever happens.

    python -I scripts/probe.py <glance.exe of a debug build> <script> [rounds]

Scripts:
    settings   the settings opened (as a second start opens them) and closed (WM_CLOSE): threads and memory after
               each, how soon the first frame came, how long it took to go
    panel      the panel opened and closed by its shortcut: how soon it showed, settled, and went
    pages      the settings opened once, then each page and the monitor's tabs turned to in turn: how soon the
               first frame of each came, and its longest frame
    resize     the settings opened once, then sized larger and smaller step by step as a drag would: each frame's
               time and the gaps between frames
    idle       nothing: the probe as it is, sampled every few seconds
"""

import ctypes
import ctypes.wintypes as wt
import os
import statistics
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, os.path.dirname(__file__))
import usage  # noqa: E402

user32 = ctypes.WinDLL("user32", use_last_error=True)
kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
psapi = ctypes.WinDLL("psapi")
WM_CLOSE, WM_HOTKEY, WM_APP = 0x0010, 0x0312, 0x8000
SHOW_SETTINGS, PERF_TURN = WM_APP + 2, WM_APP + 90
TH32CS_SNAPTHREAD = 0x4
SWP_NOZORDER, SWP_NOACTIVATE, SWP_NOMOVE = 0x4, 0x10, 0x2


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


def message_windows_of(pid, class_name):
    """Message-only windows (the tray's is a plain one; kept for windows made with HWND_MESSAGE)."""
    found = []
    after = None
    while True:
        hwnd = user32.FindWindowExW(wt.HWND(-3), after, class_name, None)
        if not hwnd:
            return found
        owner = wt.DWORD()
        user32.GetWindowThreadProcessId(hwnd, ctypes.byref(owner))
        if owner.value == pid:
            found.append(hwnd)
        after = hwnd


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
    until = time.perf_counter() + seconds
    while time.perf_counter() < until:
        found = condition()
        if found:
            return found
        time.sleep(0.005)
    return None


class Marks:
    """The probe's marks as they are written: (time, thread, what)."""

    def __init__(self, path):
        self.path = path

    def all(self):
        try:
            with open(self.path, encoding="utf-8") as file:
                lines = file.read().splitlines()
        except FileNotFoundError:
            return []
        marks = []
        for line in lines:
            parts = line.split("\t", 2)
            if len(parts) == 3:
                marks.append((float(parts[0]), parts[1], parts[2]))
        return marks

    def after(self, since, starting):
        return [mark for mark in self.all() if mark[0] >= since and mark[2].startswith(starting)]

    def first(self, since, starting, seconds=10, where=lambda mark: True):
        return wait_for(lambda: next((mark for mark in self.after(since, starting) if where(mark)), None), seconds)


def took(mark):
    """The milliseconds a timed mark (`what N ms ...`) says."""
    words = mark[2].split()
    return float(words[words.index("ms") - 1])


def summary(name, values, unit="ms"):
    if not values:
        print(f"  {name:44} none")
        return
    ordered = sorted(values)
    p95 = ordered[max(0, int(round(len(ordered) * 0.95)) - 1)]
    print(f"  {name:44} n {len(values):3}  median {statistics.median(values):8.2f}  p95 {p95:8.2f}  max {max(values):8.2f} {unit}")


def tray(pid):
    return wait_for(lambda: windows_of(pid, "GlanceTray.Debug") or message_windows_of(pid, "GlanceTray.Debug"), 20)[0]


def open_settings(exe_pid, marks):
    since = time.perf_counter()
    user32.PostMessageW(tray(exe_pid), SHOW_SETTINGS, 0, 0)
    first = marks.first(since, "settings frame", 20)
    return since, first


def close_settings(pid, marks):
    since = time.perf_counter()
    for hwnd in windows_of(pid, "GlanceSettings"):
        user32.PostMessageW(hwnd, WM_CLOSE, 0, 0)
    gone = marks.first(since, "settings gone", 20)
    return since, gone


def run_settings(pid, marks, counters, rounds):
    opening, first_frame, frames, closing = [], [], [], []
    for round_ in range(rounds):
        since, first = open_settings(pid, marks)
        if not first:
            print("  the settings did not open")
            return
        opening.append((first[0] - since) * 1000)
        first_frame.append(took(first))
        time.sleep(3)
        frames += [took(mark) for mark in marks.after(first[0] + 0.001, "settings frame")]
        opened = state(pid, counters)
        since, gone = close_settings(pid, marks)
        closing.append(((gone[0] if gone else time.perf_counter()) - since) * 1000)
        time.sleep(3)
        print(f"  round {round_ + 1}: open {opened}  |  closed {state(pid, counters)}", flush=True)
    summary("settings: asked -> first frame drawn", opening)
    summary("settings: the first frame's drawing", first_frame)
    summary("settings: frames after, open 3 s", frames)
    summary("settings: closed -> gone", closing)


def run_panel(pid, marks, rounds):
    shown, settled, frames, hidden = [], [], [], []
    for _ in range(rounds):
        since = time.perf_counter()
        user32.PostMessageW(tray(pid), WM_HOTKEY, 1, 0)
        show = marks.first(since, "panel shown", 10)
        if not show:
            print("  the panel did not open")
            return
        shown.append((show[0] - since) * 1000)
        rest = marks.first(since, "panel frame", 10, where=lambda mark: "moving=false" in mark[2] and "open" in mark[2])
        if rest:
            settled.append((rest[0] - since) * 1000)
            frames += [took(mark) for mark in marks.after(since, "panel frame") if mark[0] <= rest[0]]
        time.sleep(1.5)
        since = time.perf_counter()
        user32.PostMessageW(tray(pid), WM_HOTKEY, 1, 0)
        gone = marks.first(since, "panel hidden", 10)
        if gone:
            hidden.append((gone[0] - since) * 1000)
        time.sleep(1.0)
    summary("panel: asked -> first frame shown", shown)
    summary("panel: asked -> come to rest", settled)
    summary("panel: frames as it opens", frames)
    summary("panel: asked to close -> hidden", hidden)


def run_pages(pid, marks, rounds):
    since, first = open_settings(pid, marks)
    if not first:
        print("  the settings did not open")
        return
    time.sleep(3)
    hwnd = windows_of(pid, "GlanceSettings")[0]
    turns = [(page, 0) for page in range(7)] + [(2, 1), (2, 2), (2, 0)]
    per = {}
    for _ in range(rounds):
        for page, tab in turns:
            since = time.perf_counter()
            user32.PostMessageW(hwnd, PERF_TURN, page, tab)
            turned = marks.first(since, "settings turn", 5)
            frame = marks.first(turned[0] if turned else since, "settings frame", 5)
            time.sleep(0.6)
            if frame:
                later = [took(mark) for mark in marks.after(frame[0] - 0.0001, "settings frame") if mark[0] <= since + 0.6]
                per.setdefault((page, tab), ([], []))
                per[(page, tab)][0].append((frame[0] - since) * 1000)
                per[(page, tab)][1].append(max(later))
    names = ["layout", "look", "monitor", "workbench", "music", "phone", "general"]
    for (page, tab), (first_frames, longest) in per.items():
        summary(f"turn to {names[page]}{'/' + ['shown', 'data', 'overlay'][tab] if page == 2 else ''}: first frame", first_frames)
        summary("    its longest frame in 0.6 s", longest)
    close_settings(pid, marks)


def run_resize(pid, marks, rounds):
    since, first = open_settings(pid, marks)
    if not first:
        print("  the settings did not open")
        return
    time.sleep(3)
    hwnd = windows_of(pid, "GlanceSettings")[0]
    rect = wt.RECT()
    user32.GetWindowRect(hwnd, ctypes.byref(rect))
    width, height = rect.right - rect.left, rect.bottom - rect.top
    frames, gaps = [], []
    for _ in range(rounds):
        since = time.perf_counter()
        # As a drag: a step every 8 ms (a 125 Hz mouse), 300 px out and back.
        steps = list(range(0, 300, 5)) + list(range(300, 0, -5))
        for step in steps:
            user32.SetWindowPos(hwnd, None, 0, 0, width + step, height + step // 2, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE)
            time.sleep(0.008)
        user32.SetWindowPos(hwnd, None, 0, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE)
        until = time.perf_counter()
        drawn = [mark for mark in marks.after(since, "settings frame") if mark[0] <= until]
        frames += [took(mark) for mark in drawn]
        gaps += [(b[0] - a[0]) * 1000 for a, b in zip(drawn, drawn[1:])]
        print(f"  round: {len(steps)} sizes in {(until - since) * 1000:.0f} ms, {len(drawn)} frames drawn", flush=True)
        time.sleep(1)
    summary("resize: each frame's drawing", frames)
    summary("resize: between frames", gaps)
    close_settings(pid, marks)


def main():
    exe, script = sys.argv[1], sys.argv[2]
    rounds = int(sys.argv[3]) if len(sys.argv) > 3 else 5
    query = ctypes.c_void_p()
    usage.pdh.PdhOpenQueryW(None, 0, ctypes.byref(query))
    counters = (query, usage.Counter(query, r"\GPU Process Memory(*)\Dedicated Usage"))
    marks_path = os.path.join(tempfile.gettempdir(), f"glance-perf-{os.getpid()}.txt")
    marks = Marks(marks_path)
    probe = subprocess.Popen([exe], env={**os.environ, "GLANCE_PERF": marks_path})
    try:
        tray(probe.pid)
        time.sleep(6)
        print(f"started (pid {probe.pid}): {state(probe.pid, counters)}", flush=True)
        if script == "settings":
            run_settings(probe.pid, marks, counters, rounds)
        elif script == "panel":
            run_panel(probe.pid, marks, rounds)
        elif script == "pages":
            run_pages(probe.pid, marks, rounds)
        elif script == "resize":
            run_resize(probe.pid, marks, rounds)
        else:
            for round_ in range(rounds):
                time.sleep(5)
                print(f"after {5 * (round_ + 1)} s: {state(probe.pid, counters)}", flush=True)
        time.sleep(1)
        print(f"at the end: {state(probe.pid, counters)}")
        for wait in (5, 10, 30):
            time.sleep(wait)
            print(f"  later (+{wait} s): {state(probe.pid, counters)}", flush=True)
        if os.environ.get("PROBE_THREADS"):
            subprocess.run([sys.executable, "-I", os.path.join(os.path.dirname(__file__), "threads.py"), "glance.exe", "3"])
    finally:
        probe.kill()
        probe.wait()
        print(f"probe ended; marks in {marks_path}")


if __name__ == "__main__":
    main()
