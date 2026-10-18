"""The smoke test of the real release window, through Windows UI Automation.

Every other UI check is a headless `cargo nextest` test (crates/adeline_ui/tests
and crates/adeline/tests/ui.rs). This one covers what GPUI's test platform
can't: the native window and its close button, the title bar, real drawing, and
the accessibility tree as UI Automation sees it. It launches the release build
in demo mode in a temporary home, opens a chat and quits. Run from the repo
root after `cargo build --release --locked`:

    python scripts/engine-check/ui_check.py
"""
import os
import subprocess
import sys
import time

from pywinauto import Desktop

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import run_checks as rc  # noqa: E402


def wait(predicate, timeout=20, what="condition"):
    end = time.time() + timeout
    while time.time() < end:
        try:
            value = predicate()
            if value:
                return value
        except Exception:
            pass
        time.sleep(0.2)
    raise AssertionError(f"timed out waiting for {what}")


def find(window, name=None, control_type=None, prefix=""):
    for element in window.descendants():
        info = element.element_info
        if ((name is None or info.name == name) and (info.name or "").startswith(prefix)
                and (control_type is None or info.control_type == control_type)):
            return element
    return None


def main():
    temp = rc.setup()
    process = subprocess.Popen([str(rc.EXE), "--demo"])
    try:
        window = wait(lambda: next((w for w in Desktop(backend="uia").windows(process=process.pid)
                                    if w.is_visible()), None), 30, what="the Adeline window")
        assert window.window_text() == "Adeline", window.window_text()
        chat = wait(lambda: find(window, control_type="ListItem", prefix="Make document edits safe"),
                    15, what="a demo chat")
        chat.click_input()
        wait(lambda: find(window, "Message", "Edit"), 10, what="the message box")
        # The native close button. Demo chats still run, so Adeline may ask first.
        window.close()
        background = wait(lambda: find(window, "Finish in background", "Button")
                          or process.poll() is not None, 10, what="the quit dialog or the exit")
        if background is not True:
            background.invoke()
        wait(lambda: process.poll() is not None, 10, what="Adeline to quit")
        print("PASS  smoke test: window, chat, close button")
    except Exception:
        print("FAIL  smoke test")
        raise
    finally:
        if process.poll() is None:
            process.kill()
        print(f"Temp home: {temp}")


if __name__ == "__main__":
    main()
