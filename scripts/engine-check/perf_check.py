"""Frame-time profile of the real window.

Needs a profiling build, which logs every frame to ADELINE_FRAME_LOG:

    cargo build --profile dist --locked --features profiler --target-dir target/profiling

Then `python scripts/engine-check/perf_check.py [--exe PATH] [--only NAME] [--a11y]` runs
each scenario with the window in front and prints per-scenario frame stats. The window must
stay focused: GPUI caps unfocused windows to ~30 fps.

A UI Automation client switches on the window's accessibility tree, and while it is on
the window draws every region uncached. So a first launch finds the controls through UI
Automation, and a second, measured launch is driven by their positions alone. `--a11y`
switches the tree on in the measured launch too, as a screen reader would.
"""
import argparse
import csv
import ctypes
import os
import statistics
import sys
import time
from pathlib import Path

from pywinauto import Desktop, mouse
from pywinauto.keyboard import send_keys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_checks as rc  # noqa: E402
import ui_check as ui  # noqa: E402

LONG_CHAT = "Search long conversations without pausing the interface"
user32 = ctypes.windll.user32
kernel32 = ctypes.windll.kernel32
spans = []


def now_ms():
    return time.time() * 1000


def scenario(name, function):
    if OPTIONS.only and OPTIONS.only.lower() not in name.lower():
        return
    time.sleep(0.5)
    start, cpu = now_ms(), cpu_ms()
    function()
    spans.append((name, start, now_ms(), cpu_ms() - cpu))


def cpu_ms():
    """CPU time of the Adeline process so far, every thread included."""
    times = [ctypes.c_ulonglong() for _ in range(4)]
    kernel32.GetProcessTimes(int(PROCESS._handle), *map(ctypes.byref, times))
    return (times[2].value + times[3].value) / 10_000


def rect(element):
    r = element.rectangle()
    return r.left, r.top, r.right, r.bottom


def point(name):
    x, y = LAYOUT[name]
    return ORIGIN[0] + x, ORIGIN[1] + y


def idle():
    time.sleep(3)


def scroll():
    position = point("transcript")
    mouse.move(position)
    for direction in (-1, 1):
        for _ in range(40):
            mouse.scroll(position, direction)
            time.sleep(1 / 60)


def switch_chats():
    mouse.click(coords=point("first chat"))
    time.sleep(0.5)
    for key in ["{DOWN}"] * 8 + ["{UP}"] * 8:
        send_keys(key)
        time.sleep(0.15)


def type_text():
    mouse.click(coords=point("message"))
    send_keys("The quick brown fox jumps over the lazy dog. " * 4, with_spaces=True, pause=0.02)
    send_keys("^a{BACKSPACE}")


def hover():
    x, top = point("first chat")
    _, bottom = point("last chat")
    for _ in range(3):
        for y in list(range(top, bottom, 4)) + list(range(bottom, top, -4)):
            mouse.move((x, y))
            time.sleep(1 / 240)


def resize():
    left, top, right, bottom = ui.window_rect(HWND)
    width, height = right - left, bottom - top
    for step in list(range(0, 400, 8)) + list(range(400, 0, -8)):
        user32.SetWindowPos(HWND, 0, left, top, width - step, height, 0x0004)
        time.sleep(1 / 120)
    user32.SetWindowPos(HWND, 0, left, top, width, height, 0x0004)


def stream():
    mouse.click(coords=point("message"))
    send_keys("STREAM 1500{ENTER}", with_spaces=True)
    # A fresh home has no agent picked, so sending opens the agent picker instead.
    time.sleep(0.5)
    send_keys("Fake Auto{ENTER}", with_spaces=True)
    mouse.click(coords=point("message"))
    send_keys("{ENTER}")
    time.sleep(17)


def survey(*args):
    """Where the scenarios point, relative to the window, read through UI Automation
    from a launch of its own."""
    process, window = ui.launch(*args)
    try:
        window.set_focus()
        message = ui.named(window, "Message", timeout=20)
        time.sleep(1)
        w = window.rectangle()
        middle = lambda r: ((r[0] + r[2]) // 2 - w.left, (r[1] + r[3]) // 2 - w.top)  # noqa: E731
        items = [e for e in window.descendants() if e.element_info.control_type == "ListItem"]
        layout = {"message": middle(rect(message)), "size": (w.width(), w.height())}
        layout["transcript"] = (layout["message"][0], rect(message)[1] - w.top - 250)
        if items:
            layout["first chat"] = middle(rect(items[0]))
            layout["last chat"] = middle(rect(items[-1]))
        for e in items:
            if e.element_info.name.startswith(LONG_CHAT + ","):
                layout["long chat"] = middle(rect(e))
        return layout
    finally:
        process.kill()


def launch(log, *args):
    """Launches without UI Automation unless --a11y asks for it: a UI Automation
    client switches on the window's accessibility tree, and with it on, the window
    draws every region uncached."""
    global LAYOUT, ORIGIN, PROCESS, HWND
    LAYOUT = survey(*args)
    if not args:
        rc.stop_engine()
    os.environ["ADELINE_FRAME_LOG"] = str(log)
    PROCESS, HWND = ui.launch_blind(*args)
    time.sleep(3)
    left, top, right, bottom = ui.window_rect(HWND)
    assert (right - left, bottom - top) == LAYOUT["size"], "the window opened at another size"
    ORIGIN = left, top
    if OPTIONS.a11y:
        Desktop(backend="uia").window(handle=HWND).descendants()
    os.environ.pop("ADELINE_FRAME_LOG")
    return PROCESS


def percentile(values, fraction):
    return sorted(values)[min(len(values) - 1, int(len(values) * fraction))] if values else 0


def report(logs):
    rows = [row for log in logs if log.exists() for row in csv.DictReader(log.open())]
    print(f"{'scenario':<14}{'draws':>6}{'fps':>6}{'draw p50':>9}{'p95':>7}{'p99':>7}{'max':>7}"
          f"{'>4.2ms':>7}{'>8.3ms':>7}{'present p50':>12}{'p95':>7}{'dirty→draw p95':>15}{'CPU':>6}   (ms, CPU in % of one core)")
    for name, start, end, cpu in spans:
        inside = [r for r in rows if start <= float(r["at_ms"]) <= end]
        draws = [float(r["duration_us"]) / 1000 for r in inside if r["kind"] == "draw"]
        presents = [float(r["duration_us"]) / 1000 for r in inside if r["kind"] == "present"]
        dirty = [float(r["dirty_to_end_us"]) / 1000 for r in inside if r["kind"] == "draw" and r["dirty_to_end_us"]]
        seconds = (end - start) / 1000
        print(f"{name:<14}{len(draws):>6}{len(draws) / seconds:>6.0f}{percentile(draws, .5):>9.2f}"
              f"{percentile(draws, .95):>7.2f}{percentile(draws, .99):>7.2f}{max(draws, default=0):>7.2f}"
              f"{sum(d > 4.17 for d in draws):>7}{sum(d > 8.33 for d in draws):>7}"
              f"{percentile(presents, .5):>12.2f}{percentile(presents, .95):>7.2f}{percentile(dirty, .95):>15.2f}{cpu / (end - start) * 100:>6.0f}")
    if spans:
        print(f"\nmean draw over all scenarios: {statistics.mean(float(r['duration_us']) for r in rows if r['kind'] == 'draw') / 1000:.2f} ms")


def main():
    global OPTIONS
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=str(rc.HERE.parents[1] / "target" / "profiling" / "dist" / "adeline.exe"))
    parser.add_argument("--only")
    parser.add_argument("--a11y", action="store_true", help="switch on the accessibility tree, as a screen reader does")
    OPTIONS = parser.parse_args()
    rc.EXE = Path(OPTIONS.exe)
    temp = rc.setup()
    logs = [temp / "frames-demo.csv", temp / "frames-engine.csv"]

    process = launch(logs[0], "--demo")
    try:
        mouse.click(coords=point("long chat"))
        scenario("idle", idle)
        scenario("scroll", scroll)
        scenario("switch chats", switch_chats)
        scenario("typing", type_text)
        scenario("hover list", hover)
        scenario("resize", resize)
    finally:
        time.sleep(0.3)
        process.kill()

    if not OPTIONS.only or "stream" in OPTIONS.only.lower():
        rc.stop_engine()
        process = launch(logs[1])
        try:
            scenario("stream", stream)
        finally:
            time.sleep(0.3)
            process.kill()
            rc.stop_engine()
    time.sleep(0.3)
    report(logs)
    print(f"\nframe logs: {temp}")


OPTIONS = None
PROCESS = None
HWND = None
LAYOUT = None
ORIGIN = None

if __name__ == "__main__":
    main()
