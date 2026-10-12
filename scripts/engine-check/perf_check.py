"""Frame-time profile of the real window, through UI Automation.

Needs a profiling build, which logs every frame to ADELINE_FRAME_LOG:

    cargo build --profile dist --locked --features profiler --target-dir target/profiling

Then `python scripts/engine-check/perf_check.py [--exe PATH] [--only NAME]` runs each
scenario with the window in front and prints per-scenario frame stats. The window must
stay focused: GPUI caps unfocused windows to ~30 fps.
"""
import argparse
import csv
import ctypes
import os
import statistics
import sys
import time
from pathlib import Path

from pywinauto import mouse
from pywinauto.keyboard import send_keys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_checks as rc  # noqa: E402
import ui_check as ui  # noqa: E402

LONG_CHAT = "Search long conversations without pausing the interface"
user32 = ctypes.windll.user32
spans = []


def now_ms():
    return time.time() * 1000


def scenario(name, function, *args):
    if OPTIONS.only and OPTIONS.only.lower() not in name.lower():
        return
    time.sleep(0.5)
    start = now_ms()
    function(*args)
    spans.append((name, start, now_ms()))


def rect(element):
    r = element.rectangle()
    return r.left, r.top, r.right, r.bottom


def idle(_window):
    time.sleep(3)


def scroll(window):
    left, top, right, _ = rect(ui.named(window, "Message"))
    point = ((left + right) // 2, top - 250)
    mouse.move(point)
    for direction in (-1, 1):
        for _ in range(40):
            mouse.scroll(point, direction)
            time.sleep(1 / 60)


def switch_chats(window):
    next(e for e in window.descendants() if e.element_info.control_type == "ListItem").click_input()
    time.sleep(0.5)
    for key in ["{DOWN}"] * 8 + ["{UP}"] * 8:
        send_keys(key)
        time.sleep(0.15)


def type_text(window):
    ui.named(window, "Message").click_input()
    send_keys("The quick brown fox jumps over the lazy dog. " * 4, with_spaces=True, pause=0.02)
    send_keys("^a{BACKSPACE}")


def hover(window):
    items = [rect(e) for e in window.descendants() if e.element_info.control_type == "ListItem"]
    left, top, right, bottom = items[0][0], items[0][1], items[0][2], items[-1][3]
    x = (left + right) // 2
    for _ in range(3):
        for y in list(range(top, bottom, 4)) + list(range(bottom, top, -4)):
            mouse.move((x, y))
            time.sleep(1 / 240)


def resize(window):
    handle = window.handle
    left, top, right, bottom = rect(window)
    width, height = right - left, bottom - top
    for step in list(range(0, 400, 8)) + list(range(400, 0, -8)):
        user32.SetWindowPos(handle, 0, left, top, width - step, height, 0x0004)
        time.sleep(1 / 120)
    user32.SetWindowPos(handle, 0, left, top, width, height, 0x0004)


def stream(window):
    ui.send(window, "STREAM 1500")
    # A fresh home has no agent picked, so sending opens the agent picker instead.
    time.sleep(0.5)
    send_keys("Fake Auto{ENTER}", with_spaces=True)
    ui.named(window, "Message").click_input()
    send_keys("{ENTER}")
    time.sleep(17)


def launch(log, *args):
    os.environ["ADELINE_FRAME_LOG"] = str(log)
    process, window = ui.launch(*args)
    window.set_focus()
    ui.named(window, "Message", timeout=20)
    time.sleep(2)
    return process, window


def percentile(values, fraction):
    return sorted(values)[min(len(values) - 1, int(len(values) * fraction))] if values else 0


def report(logs):
    rows = [row for log in logs if log.exists() for row in csv.DictReader(log.open())]
    print(f"{'scenario':<14}{'draws':>6}{'fps':>6}{'draw p50':>9}{'p95':>7}{'p99':>7}{'max':>7}"
          f"{'>4.2ms':>7}{'>8.3ms':>7}{'present p50':>12}{'p95':>7}{'dirty→draw p95':>15}   (ms)")
    for name, start, end in spans:
        inside = [r for r in rows if start <= float(r["at_ms"]) <= end]
        draws = [float(r["duration_us"]) / 1000 for r in inside if r["kind"] == "draw"]
        presents = [float(r["duration_us"]) / 1000 for r in inside if r["kind"] == "present"]
        dirty = [float(r["dirty_to_end_us"]) / 1000 for r in inside if r["kind"] == "draw" and r["dirty_to_end_us"]]
        seconds = (end - start) / 1000
        print(f"{name:<14}{len(draws):>6}{len(draws) / seconds:>6.0f}{percentile(draws, .5):>9.2f}"
              f"{percentile(draws, .95):>7.2f}{percentile(draws, .99):>7.2f}{max(draws, default=0):>7.2f}"
              f"{sum(d > 4.17 for d in draws):>7}{sum(d > 8.33 for d in draws):>7}"
              f"{percentile(presents, .5):>12.2f}{percentile(presents, .95):>7.2f}{percentile(dirty, .95):>15.2f}")
    if spans:
        print(f"\nmean draw over all scenarios: {statistics.mean(float(r['duration_us']) for r in rows if r['kind'] == 'draw') / 1000:.2f} ms")


def main():
    global OPTIONS
    parser = argparse.ArgumentParser()
    parser.add_argument("--exe", default=str(rc.HERE.parents[1] / "target" / "profiling" / "dist" / "adeline.exe"))
    parser.add_argument("--only")
    OPTIONS = parser.parse_args()
    rc.EXE = Path(OPTIONS.exe)
    temp = rc.setup()
    logs = [temp / "frames-demo.csv", temp / "frames-engine.csv"]

    process, window = launch(logs[0], "--demo")
    try:
        ui.open_chat(window, LONG_CHAT)
        scenario("idle", idle, window)
        scenario("scroll", scroll, window)
        scenario("switch chats", switch_chats, window)
        scenario("typing", type_text, window)
        scenario("hover list", hover, window)
        scenario("resize", resize, window)
    finally:
        time.sleep(0.3)
        process.kill()

    if not OPTIONS.only or "stream" in OPTIONS.only.lower():
        rc.stop_engine()
        process, window = launch(logs[1])
        try:
            scenario("stream", stream, window)
        finally:
            time.sleep(0.3)
            process.kill()
            rc.stop_engine()
    time.sleep(0.3)
    report(logs)
    print(f"\nframe logs: {temp}")


OPTIONS = None

if __name__ == "__main__":
    main()
