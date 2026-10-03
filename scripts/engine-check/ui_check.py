"""UI checks for the conversation engine, through Windows UI Automation.

Starts the real Adeline window against an isolated temporary home and drives
it with pywinauto: the engine starts as its own process, the quit dialog's
Cancel / Finish in background / Stop all, Settings > Engine, Stop engine and
Start engine, and demo mode without an engine. Run from the repo root:

    python scripts/engine-check/ui_check.py
"""
import os
import subprocess
import sys
import time
import traceback

from pywinauto import Desktop
from pywinauto.keyboard import send_keys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_checks as rc  # noqa: E402
from engine_client import Client  # noqa: E402

results = []


def wait(predicate, timeout=20, interval=0.2, what="condition"):
    end = time.time() + timeout
    while time.time() < end:
        try:
            value = predicate()
            if value:
                return value
        except Exception:
            pass
        time.sleep(interval)
    raise AssertionError(f"timed out waiting for {what}")


def engine_pid():
    result = rc.adeline("engine", "status", timeout=20)
    if result.returncode != 0:
        return None
    for line in result.stdout.splitlines():
        if line.startswith("PID:"):
            return int(line.split(":")[1])
    return None


def launch(*args):
    process = subprocess.Popen([str(rc.EXE), *args])
    window = wait(lambda: next((w for w in Desktop(backend="uia").windows(process=process.pid) if w.is_visible()), None),
                  30, what="the Adeline window")
    return process, window


def find(window, name, control_type=None):
    for element in window.descendants():
        info = element.element_info
        if info.name == name and (control_type is None or info.control_type == control_type):
            return element
    return None


def named(window, name, control_type=None, timeout=15):
    return wait(lambda: find(window, name, control_type), timeout, what=f"'{name}'")


def gone(window, name):
    return find(window, name) is None


def click(window, name, control_type="Button"):
    element = named(window, name, control_type)
    try:
        element.invoke()
    except Exception:
        element.click_input()


def check(name):
    def register(function):
        def run():
            start = time.time()
            try:
                note = function()
                results.append(True)
                print(f"PASS  {name}  ({time.time() - start:.1f} s){'  ' + note if note else ''}", flush=True)
            except Exception:
                results.append(False)
                print(f"FAIL  {name}  ({time.time() - start:.1f} s)", flush=True)
                print("      " + traceback.format_exc().replace("\n", "\n      "), flush=True)
        return run
    return register


def send(window, prompt):
    named(window, "Message").click_input()
    send_keys(prompt, with_spaces=True)
    send_keys("{ENTER}")


def processing_conversation():
    with Client() as client:
        found = next((id for id, live in client.snapshot["live"].items() if live.get("processing")), None)
    if found:
        # This check's own connection counted as a client; let the UI see it leave.
        def clients():
            with rc.Client(cli=True) as control:
                return control.call({"op": "status"})["clients"]
        wait(lambda: clients() == 1, 10, what="one connected client")
        time.sleep(0.5)
    return found


@check("AC1/AC25: UI starts the engine as a separate process and shows the project")
def check_launch():
    rc.stop_engine()
    process, window = launch()
    try:
        named(window, "demo", timeout=10)
        pid = wait(engine_pid, 10, what="a running engine")
        assert pid != process.pid, "the engine runs in the UI process"
        return f"UI {process.pid}, engine {pid}"
    finally:
        process.kill()


@check("AC5/AC7/AC9: quit dialog, Cancel, Finish in background, reopen")
def check_quit_background():
    rc.stop_engine()
    process, window = launch()
    try:
        named(window, "demo", timeout=10)
        send(window, "SLOW 6")
        conversation = wait(processing_conversation, 15, what="a processing turn")
        window.close()
        named(window, "Finish in background", timeout=10)
        named(window, "Stop all")
        click(window, "Cancel")
        time.sleep(1)
        assert process.poll() is None, "Cancel closed Adeline"
        window.close()
        click(window, "Finish in background")
        wait(lambda: process.poll() is not None, 15, what="Adeline to quit")
        pid = engine_pid()
        assert pid, "the engine stopped with the UI"
        # The turn finishes with no window open.
        wait(lambda: "turn_finished" in str(rc.transcript(conversation)), 30, what="the background turn to finish")
        process, window = launch()
        named(window, "demo", timeout=10)
        assert engine_pid() == pid, "reopening started another engine"
        return f"engine {pid} kept the turn running"
    finally:
        if process.poll() is None:
            process.kill()


@check("AC5: quitting with nothing active shows no dialog")
def check_quit_idle():
    process, window = launch()
    named(window, "demo", timeout=10)
    window.close()
    wait(lambda: process.poll() is not None, 10, what="Adeline to quit without asking")


@check("AC6: Stop all from the quit dialog stops agents, then quits")
def check_quit_stop_all():
    process, window = launch()
    try:
        named(window, "demo", timeout=10)
        send(window, "SLOW 30")
        wait(processing_conversation, 15, what="a processing turn")
        window.close()
        click(window, "Stop all")
        wait(lambda: process.poll() is not None, 15, what="Adeline to quit after Stop all")
        with Client() as client:
            busy = [id for id, live in client.snapshot["live"].items() if live.get("running") or live.get("processing")]
        assert not busy, f"agents still running: {busy}"
    finally:
        if process.poll() is None:
            process.kill()


@check("AC14/AC23: Settings > Engine, Stop engine, Start engine")
def check_settings_engine():
    process, window = launch()
    try:
        named(window, "demo", timeout=10)
        window.set_focus()
        send_keys("^,")
        settings = wait(lambda: next(w for w in Desktop(backend="uia").windows(process=process.pid)
                                     if find(w, "Search settings")),
                        10, what="the Settings window")
        click(settings, "Show Engine")
        click(settings, "Engine")
        named(settings, "Keep conversation engine running")
        named(settings, "Automatic retry limit")
        click(settings, "Stop engine")
        wait(lambda: engine_pid() is None, 15, what="the engine to stop")
        named(window, "Conversation engine stopped", timeout=10)
        time.sleep(2)
        assert engine_pid() is None, "the UI restarted the engine on its own"
        click(window, "Start engine")
        wait(engine_pid, 15, what="the engine to start again")
        settings.close()
    finally:
        process.kill()


@check("AC24: an engine that can't start shows the unavailable state with Retry")
def check_unavailable():
    rc.stop_engine()
    engine_dir = rc.config / "engine"
    moved = rc.config / "engine-moved"
    engine_dir.rename(moved)
    # A file where the engine's folder belongs stops it from starting.
    engine_dir.write_text("not a folder", encoding="utf-8")
    process, window = launch()
    try:
        named(window, "Conversation engine unavailable", timeout=25)
        named(window, "Retry", "Button")
        assert engine_dir.read_text(encoding="utf-8") == "not a folder", "engine files were written"
        assert engine_pid() is None
    finally:
        process.kill()
        engine_dir.unlink()
        moved.rename(engine_dir)


@check("AC26: demo mode never starts the engine")
def check_demo():
    rc.stop_engine()
    process, window = launch("--demo")
    try:
        time.sleep(3)
        assert engine_pid() is None, "demo mode started an engine"
    finally:
        process.kill()


def main():
    temp = rc.setup()
    # One agent, so new chats pick it without a menu.
    for folder in ("fake",):
        path = rc.config / "agents" / folder / "agent.yml"
        path.unlink()
        path.parent.rmdir()
    try:
        for run in (check_launch, check_quit_background, check_quit_idle, check_quit_stop_all,
                    check_settings_engine, check_unavailable, check_demo):
            run()
    finally:
        rc.stop_engine()
        print(f"Temp home: {temp}")
    failed = results.count(False)
    print(f"{len(results) - failed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
