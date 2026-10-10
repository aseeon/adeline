"""UI checks for the conversation engine, through Windows UI Automation.

Starts the real Adeline window against an isolated temporary home and drives
it with pywinauto: the engine starts as its own process, the quit dialog's
Cancel / Finish in background / Stop all, Settings > Engine, Stop engine and
Start engine, and demo mode without an engine. Run from the repo root:

    python scripts/engine-check/ui_check.py
"""
import contextlib
import os
import re
import socket
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
        run.label = name
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


def count(window, name):
    return sum(element.element_info.name == name for element in window.descendants())


@check("Fork AC1/AC2/AC5/AC7/AC12: Fork opens a copy that links back to its source, also in demo mode")
def check_fork():
    process, window = launch()
    try:
        # The tab reads "demo (1)" once an earlier check leaves a chat needing input.
        named(window, "Message", timeout=30)
        send(window, "hello")
        named(window, "Fork", "Button", timeout=20)
        click(window, "Fork")
        named(window, "Forked from hello", timeout=10)
        with Client() as client:
            titles = [t["title"] for p in client.snapshot["projects"] for t in p["threads"]]
        assert "hello (fork)" in titles, titles
        send(window, "after")
        wait(lambda: count(window, "Fork") == 2, 20, what="the fork's own reply")
        assert find(window, "Forked from hello"), "a native fork showed the text-copy note"
        click(window, "hello", "Hyperlink")
        wait(lambda: gone(window, "Forked from hello"), 10, what="the source to open")
    finally:
        process.kill()
    # AC12: demo mode forks locally and starts no engine.
    rc.stop_engine()
    process, window = launch("--demo")
    try:
        chat = wait(lambda: next(e for e in window.descendants() if e.element_info.control_type == "ListItem"),
                    15, what="a demo chat")
        chat.click_input()
        click(window, "Fork")
        named(window, "Forked from " + chat.element_info.name.split(",")[0], timeout=10)
        assert engine_pid() is None, "a demo fork started the engine"
    finally:
        process.kill()


def open_chat(window, title):
    """Opens a chat by its title. The chat list has no shortcut to a chat by name, so this clicks."""
    item = wait(lambda: next(e for e in window.descendants() if e.element_info.control_type == "ListItem"
                             and e.element_info.name.startswith(title + ",")), 15, what=f"chat '{title}'")
    item.click_input()


@check("ACP rebuild AC (demo): TODO list, queue, traffic tab, hang notice, option menus, dismissing an error")
def check_demo_conversation():
    rc.stop_engine()
    process, window = launch("--demo")
    try:
        open_chat(window, "Search long conversations without pausing the interface")
        named(window, "TODO: Check matches across two messages, 3 of 4", "Button")
        send_keys("^+t")
        named(window, "TODO", "List", timeout=5)
        send_keys("{ESC}")
        named(window, "Queued messages")
        named(window, "Queued: Also keep the match count visible while typing.")
        send_keys("^+l")
        named(window, "ACP traffic", timeout=5)
        named(window, "Copy all", "Button", timeout=5)
        # A turn with no traffic for longer than the notice delay (10 min) offers Stop and Restart.
        open_chat(window, "Import sessions from the old workspace format")
        named(window, "Agent silent for 12 min")
        # An idle chat's menus open from the keyboard (Alt, since Ctrl+Shift+M/E/O are
        # often global hotkeys of GPU tools) and list the agent's choices.
        open_chat(window, "Make document edits safe when the preview is still updating")
        named(window, "Model: Opus", "Button")
        # Menu items aren't exposed to UI Automation; search, pick with Enter, read the trigger.
        send_keys("%m")
        named(window, "Search model", timeout=5)
        send_keys("Sonnet{ENTER}")
        named(window, "Model: Sonnet", "Button", timeout=5)
        send_keys("%o")
        named(window, "Search mode", timeout=5)
        send_keys("Accept{ENTER}")
        named(window, "Mode: Accept edits", "Button", timeout=5)
        send_keys("%p")
        named(window, "Search more options", timeout=5)
        send_keys("{ESC}")
        wait(lambda: gone(window, "Search more options"), 5, what="More options to close")
        # An error under a chat can be dismissed by hand.
        open_chat(window, "Keep the composer usable in a narrow desktop window")
        click(window, "Dismiss error")
        wait(lambda: gone(window, "Dismiss error"), 5, what="the error to go")
        assert engine_pid() is None, "demo mode started an engine"
    finally:
        process.kill()


# ---------------------------------------------------------------------------
# Remote machines, through fake_ssh.py: each destination is a separate home
# with its own engine on this computer.

REMOTES = None


@contextlib.contextmanager
def home_of(path):
    """Points engine_client at another home's engine."""
    saved = os.environ["USERPROFILE"]
    os.environ["USERPROFILE"] = str(path)
    try:
        yield
    finally:
        os.environ["USERPROFILE"] = saved


def make_remote(host, project="app"):
    """A remote home with the fake agent and one project."""
    saved = rc.config
    rc.config = REMOTES / host / ".config" / "adeline"
    try:
        rc.write_agent("fake-auto", "Fake Auto")
        work = REMOTES / f"{host}-work"
        work.mkdir(parents=True, exist_ok=True)
        rc.write_project(project, project, work)
    finally:
        rc.config = saved


def remote_status(host):
    with home_of(REMOTES / host):
        try:
            with Client(cli=True, timeout=2) as client:
                return client.call({"op": "status"}, 5)
        except Exception:
            return None


def stop_remotes():
    for home in REMOTES.iterdir() if REMOTES.exists() else []:
        if home.is_dir() and remote_status(home.name):
            with home_of(home):
                try:
                    with Client(cli=True, timeout=2) as client:
                        client.request({"op": "shutdown"}, 30)
                except Exception:
                    pass


def remote_transcript(host, conversation):
    path = REMOTES / host / ".config" / "adeline" / "projects" / "app" / "conversations" / conversation
    return (path / "transcript.jsonl").read_text(encoding="utf-8") if path.exists() else ""


def remote_conversations(host):
    folder = REMOTES / host / ".config" / "adeline" / "projects" / "app" / "conversations"
    return [p.name for p in folder.iterdir()] if folder.exists() else []


def save_machines(*machines, checked=None):
    """machines.yml with (id, name, destinations) entries."""
    lines = ["machines:"]
    for id, name, destinations in machines:
        lines += [f"- id: {id}", f"  name: {name}", "  destinations:"] + [f"  - {d}" for d in destinations]
    ids = ["local"] + [m[0] for m in machines] if checked is None else checked
    lines += ["checked:"] + [f"- {id}" for id in ids]
    (rc.config / "machines.yml").write_text("\n".join(lines) + "\n", encoding="utf-8")


def bridges():
    """`adeline bridge` processes: an adeline.exe that cmd.exe started."""
    table = rc.processes()
    return [pid for pid, (parent, _, exe) in table.items()
            if exe.lower() == "adeline.exe" and table.get(parent, (0, 0, ""))[2].lower() == "cmd.exe"]


def containing(window, text, control_type=None):
    for element in window.descendants():
        info = element.element_info
        if text in (info.name or "") and (control_type is None or info.control_type == control_type):
            return element
    return None


def tabs(window):
    """Tab names without the "(n)" of chats needing the user."""
    names = [e.element_info.name for e in window.descendants() if e.element_info.control_type == "TabItem"]
    return [re.sub(r" \(\d+\)", "", name) for name in names]


def tab(window, name):
    """The tab named `name`, whatever its attention count."""
    return next(e for e in window.descendants()
                if e.element_info.control_type == "TabItem" and re.sub(r" \(\d+\)", "", e.element_info.name) == name)


def selector(window):
    """Opens the machine selector; returns {machine: row text}."""
    if not find(window, "Machines", "List"):
        named(window, "Machines", "Button").click_input()
    rows = named(window, "Machines", "List")
    return {name.split(", ")[0]: name for e in rows.descendants()
            if e.element_info.control_type == "ListItem" and ", " in (name := e.element_info.name or "")}


def close_menu(window):
    send_keys("{ESC}")
    time.sleep(0.3)


def settings_window(process, window):
    window.set_focus()
    send_keys("^,")
    return wait(lambda: next(w for w in Desktop(backend="uia").windows(process=process.pid)
                             if find(w, "Search settings")), 10, what="the Settings window")


def choose_machine(settings, index):
    """Picks the dropdown entry at `index`: the machines in order, then "Add
    machine…" (-2) and "Manage machines…" (-1). The items carry no UIA names."""
    named(settings, "Machine").click_input()
    items = wait(lambda: [e for e in settings.descendants() if e.element_info.control_type == "ListItem"],
                 10, what="the machine dropdown")
    items[index].click_input()


def type_into(window, name, text):
    named(window, name, "Edit").click_input()
    send_keys("^a{BACKSPACE}")
    send_keys(text, with_spaces=True)


@check("Remote AC36: demo machines, labels, checking, the disconnected look and the settings dropdown")
def check_demo_machines():
    rc.stop_engine()
    process, window = launch("--demo")
    try:
        # Tabs read "name · machine (needs you)", and ", disconnected" for a dropped machine.
        wait(lambda: any("· Matrix" in t for t in tabs(window)), 15, what="a Matrix tab")
        assert any("· Vortex" in t and t.endswith(", disconnected") for t in tabs(window)), tabs(window)
        rows = selector(window)
        assert rows.keys() == {"Nexus", "Matrix", "Vortex"}, rows
        assert "Vortex, Disconnected" in rows.values() and "Matrix, Connected" in rows.values(), rows
        named(window, "Show Matrix").click_input()
        wait(lambda: not any("Matrix" in t for t in tabs(window)), 10, what="the Matrix tab to go")
        named(window, "Show Matrix").click_input()
        wait(lambda: any("Matrix" in t for t in tabs(window)), 10, what="the Matrix tab to return")
        close_menu(window)
        settings = settings_window(process, window)
        assert find(settings, "Show General") or find(settings, "Hide General")
        choose_machine(settings, 1)
        wait(lambda: not (find(settings, "Show General") or find(settings, "Hide General")), 10,
             what="General to hide for a remote machine")
        assert find(settings, "Show Engine") or find(settings, "Hide Engine")
        choose_machine(settings, 0)
        wait(lambda: find(settings, "Show General") or find(settings, "Hide General"), 10,
             what="General to return for the local machine")
        settings.close()
        assert engine_pid() is None, "demo mode started an engine"
    finally:
        process.kill()


@check("Remote AC5/AC1/AC25/AC26/AC7/AC10/AC12: add a machine, Adeline installs itself there, a prompt runs there")
def check_add_machine():
    rc.stop_engine()
    make_remote("desktop")
    rc.write_project("app", "app", rc.work)
    (rc.config / "machines.yml").unlink(missing_ok=True)
    process, window = launch()
    try:
        wait(lambda: "demo" in tabs(window), 15, what="the demo tab")
        assert not find(window, "Machines", "Button"), "the selector shows with no remote machine"
        settings = settings_window(process, window)
        choose_machine(settings, -2)
        type_into(settings, "Machine name", "Desktop")
        type_into(settings, "Destination", "me@desktop")
        click(settings, "Add machine")
        named(window, "Machines", "Button", timeout=15)
        wait(lambda: "app · Desktop" in tabs(window), 60, what="the remote project's tab")
        assert f"app · {socket.gethostname().split('.')[0]}" in tabs(window), tabs(window)
        assert (REMOTES / "desktop" / ".adeline" / "bin" / "adeline.exe").is_file(), "nothing was installed"
        settings.close()
        tab(window, "app · Desktop").click_input()
        send(window, "hello")
        conversation = wait(lambda: remote_conversations("desktop")[0], 20, what="a remote conversation")
        wait(lambda: "turn_finished" in remote_transcript("desktop", conversation), 30,
             what="the remote turn to finish")
        assert not (rc.work / ".config").exists()
        machines = (rc.config / "machines.yml").read_text(encoding="utf-8")
        assert "engine: " in machines, machines
        return f"remote engine {remote_status('desktop')['pid']}"
    finally:
        process.kill()


@check("Remote AC30/AC33/AC3/AC4: a dropped connection keeps data, reconnects and catches up; unchecking disconnects")
def check_drop():
    rc.stop_engine()
    process, window = launch()
    try:
        wait(lambda: "app · Desktop" in tabs(window), 60, what="the remote tab after a restart")
        tab(window, "app · Desktop").click_input()
        named(window, "New chat").click_input()
        send(window, "SLOW 6")
        before = set(remote_conversations("desktop"))
        wait(lambda: remote_status("desktop")["conversations"], 20, what="a remote turn")
        conversation = (set(remote_conversations("desktop")) - before or set(remote_conversations("desktop"))).pop()
        for pid in bridges():
            rc.kill(pid)
        wait(lambda: containing(window, "Desktop is disconnected"), 10, what="the disconnected banner")
        assert "app · Desktop, disconnected" in tabs(window) or any("disconnected" in t for t in tabs(window))
        wait(lambda: not containing(window, "Desktop is disconnected"), 30, what="the reconnect")
        wait(lambda: "turn_finished" in remote_transcript("desktop", conversation), 30, what="the turn")
        wait(lambda: not remote_status("desktop")["conversations"], 10, what="the remote turn to end")
        rows = selector(window)
        assert rows["Desktop"] == "Desktop, Connected", rows
        named(window, "Show Desktop").click_input()
        wait(lambda: not any("Desktop" in t for t in tabs(window)), 10, what="Desktop's tab to go")
        wait(lambda: remote_status("desktop")["clients"] == 0, 10, what="the connection to close")
        close_menu(window)
        process.kill()
        process, window = launch()
        wait(lambda: "demo" in tabs(window), 15, what="the demo tab")
        time.sleep(2)
        assert not any("Desktop" in t for t in tabs(window)), "an unchecked machine came back on restart"
        selector(window)
        named(window, "Show Desktop").click_input()
        wait(lambda: "app · Desktop" in tabs(window), 60, what="Desktop's tab to return")
    finally:
        process.kill()


@check("Remote AC20/AC21/AC22/AC23/AC24/AC31/AC32/AC6: SSH prompts, refusals and failure states")
def check_ssh_failures():
    rc.stop_engine()
    (REMOTES / "pw-locked.alias").write_text("locked")
    (REMOTES / "newkey-fresh.alias").write_text("fresh")
    (REMOTES / "twin.alias").write_text("desktop")
    (REMOTES / "other.alias").write_text("other")
    desktop = (rc.config / "machines.yml").read_text(encoding="utf-8")
    engine = desktop.split("engine: ")[1].split()[0]
    save_machines(("m-locked", "Locked", ["pw-locked"]), ("m-wrong", "Wrong", ["pw-wrong"]),
                  ("m-away", "Away", ["down-away"]), ("m-moved", "Moved", ["changed-moved"]))
    process, window = launch()
    try:
        answered = {"Locked": 0, "Wrong": 0}

        def answer_prompts():
            title = containing(window, "Sign in to ")
            if title:
                machine = title.element_info.name.removeprefix("Sign in to ")
                answered[machine] += 1
                type_into(window, "SSH answer", "secret" if machine == "Locked" and answered[machine] > 1 else "nope")
                click(window, "Continue")
                time.sleep(1)
                return False
            rows = selector(window)
            done = rows.get("Locked") == "Locked, Connected" and rows.get("Wrong") == "Wrong, Sign-in failed"
            close_menu(window)
            return done

        wait(answer_prompts, 90, interval=0.5, what="Locked to sign in and Wrong to fail")
        asked = answered["Wrong"]
        time.sleep(6)
        assert not containing(window, "Sign in to Wrong") and answered["Wrong"] == asked, "Wrong kept retrying"
        rows = selector(window)
        assert rows["Away"] == "Away, Disconnected", rows
        assert containing(window, "Connection timed out"), "the timeout isn't shown"
        assert rows["Moved"] == "Moved, Disconnected" and containing(window, "REMOTE HOST IDENTIFICATION"), rows
        close_menu(window)
        assert "secret" not in (rc.config / "machines.yml").read_text(encoding="utf-8")
        process.kill()
        # Host keys, another engine at a destination, and an engine saved twice.
        save_machines(("m-fresh", "Fresh", ["newkey-fresh"]), ("m-desktop", "Desktop", ["other", "desktop"]),
                      ("m-twin", "Twin", ["twin"]))
        text = (rc.config / "machines.yml").read_text(encoding="utf-8")
        text = text.replace("  - other\n  - desktop\n", f"  - other\n  - desktop\n  engine: {engine}\n")
        (rc.config / "machines.yml").write_text(text, encoding="utf-8")
        make_remote("fresh")
        process, window = launch()
        named(window, "Trust Fresh?", timeout=30)
        click(window, "Reject")
        wait(lambda: selector(window).get("Fresh") == "Fresh, Disconnected", 15, what="Fresh to be refused")
        assert containing(window, "Host key verification failed")
        named(window, "Retry").click_input()
        named(window, "Trust Fresh?", timeout=30)
        click(window, "Accept")
        wait(lambda: "app · Fresh" in tabs(window), 60, what="Fresh to connect")
        wait(lambda: "app · Desktop" in tabs(window), 60, what="Desktop through its second destination")
        assert not remote_status("other"), "a refused destination's engine was started"
        wait(lambda: "Twin" not in (rc.config / "machines.yml").read_text(encoding="utf-8"), 30,
             what="Twin to be refused")
        return "Twin refused as a second Desktop"
    finally:
        process.kill()


OLD_RELEASE = "https://github.com/aseeon/adeline/releases/download/v0.1.0/adeline-windows-x86_64.zip"


def old_adeline():
    """Adeline 0.1.0 from its release, cached in the temp folder; None offline."""
    import tempfile
    import urllib.request
    import zipfile
    exe = os.path.join(tempfile.gettempdir(), "adeline-ui-check-0.1.0", "adeline.exe")
    if not os.path.exists(exe):
        try:
            archive, _ = urllib.request.urlretrieve(OLD_RELEASE)
            zipfile.ZipFile(archive).extractall(os.path.dirname(exe))
        except OSError:
            return None
    return exe


@check("Remote AC28/AC27/AC29/AC12/AC13: upgrade prompt, a newer Adeline on the same protocol, unsupported, browsing remote folders")
def check_upgrade_and_browse():
    rc.stop_engine()
    old = old_adeline()
    assert old, f"couldn't download {OLD_RELEASE}"
    (REMOTES / "old").mkdir(exist_ok=True)
    started = subprocess.run([old, "engine", "start"], capture_output=True, text=True, timeout=30,
                             env={**os.environ, "USERPROFILE": str(REMOTES / "old"), "HOME": str(REMOTES / "old")})
    assert started.returncode == 0, started.stdout + started.stderr
    newer = REMOTES / "newer" / ".adeline" / "bin"
    newer.mkdir(parents=True, exist_ok=True)
    # A newer Adeline that speaks this client's protocol is used as it is.
    (newer / "adeline.exe").write_bytes(rc.EXE.read_bytes())
    (newer / "version").write_text("99.0.0")
    (REMOTES / "desktop" / "code" / "site").mkdir(parents=True, exist_ok=True)
    text = (rc.config / "machines.yml").read_text(encoding="utf-8")
    engine = text.split("engine: ")[-1].split()[0]
    save_machines(("m-desktop", "Desktop", ["desktop"]), ("m-old", "Old", ["old"]),
                  ("m-newer", "Newer", ["newer"]), ("m-board", "Board", ["arm-board"]))
    text = (rc.config / "machines.yml").read_text(encoding="utf-8")
    (rc.config / "machines.yml").write_text(text.replace("  - desktop\n", f"  - desktop\n  engine: {engine}\n"),
                                            encoding="utf-8")
    process, window = launch()
    try:
        named(window, "Upgrade Old?", "Group", timeout=60)
        click(window, "Later")
        rows = wait(lambda: (r := selector(window)).get("Board") == "Board, Unsupported"
                    and r.get("Newer") == "Newer, Connected" and r, 60,
                    what="every machine's state")
        assert rows["Old"] == "Old, Upgrade needed", rows
        assert (newer / "version").read_text() == "99.0.0", "the newer Adeline was replaced"
        assert containing(window, "No Adeline build for windows-arm64"), "the unsupported platform isn't named"
        assert containing(window, "It runs Adeline 0.1.0 with 0 active conversations"), "the old version isn't named"
        assert remote_status("old")["version"] == "0.1.0", "Later replaced the old engine"
        click(window, "Upgrade…")
        named(window, "Upgrade Old?", "Group", timeout=10)
        click(window, "Upgrade now (stops them)")
        wait(lambda: (s := remote_status("old")) and s["version"] != "0.1.0", 60, what="the new engine on Old")
        wait(lambda: selector(window).get("Old") == "Old, Connected", 30, what="Old to connect")
        close_menu(window)
        # Open folder on a remote machine browses its folders.
        wait(lambda: "app · Desktop" in tabs(window), 60, what="Desktop's project")

        def open_site():
            named(window, "Projects", "Button").click_input()
            click(window, "Open folder")
            named(window, "Open a folder on which machine?", "Group", timeout=10)
            click(window, "Desktop")
            named(window, "Open folder on Desktop", "Group", timeout=15)
            named(window, "code/", "ListItem").click_input()
            named(window, "site/", "ListItem", timeout=10).click_input()
            named(window, "Up", "Button", timeout=10)
            wait(lambda: not find(window, "site/", "ListItem"), 10, what="the site folder to open")
            click(window, "Choose this folder")

        open_site()
        wait(lambda: "site · Desktop" in tabs(window), 20, what="the new remote project")
        with home_of(REMOTES / "desktop"):
            with Client() as client:
                sites = [p for p in client.snapshot["projects"] if p["config"]["name"] == "site"]
        assert len(sites) == 1 and sites[0]["config"]["directory"] == str(REMOTES / "desktop" / "code" / "site"), sites
        open_site()
        time.sleep(2)
        assert tabs(window).count("site · Desktop") == 1, tabs(window)
        with home_of(REMOTES / "desktop"):
            with Client() as client:
                count_sites = sum(p["config"]["name"].startswith("site") for p in client.snapshot["projects"])
        assert count_sites == 1, "opening the same folder again made another project"
    finally:
        process.kill()


# The headless build, beside the full one: `cargo build --release --locked
# --no-default-features --target-dir target/headless`.
HEADLESS = rc.EXE.parents[1] / "headless" / "release" / "adeline.exe"


def headless(*args, home=None):
    env = {**os.environ, "USERPROFILE": str(home), "HOME": str(home)} if home else None
    return subprocess.run([str(HEADLESS), *args], capture_output=True, text=True, timeout=30, env=env)


def windows_subsystem(exe):
    """The PE subsystem: 2 for a GUI program, 3 for a console one."""
    data = exe.read_bytes()
    pe = int.from_bytes(data[0x3C:0x40], "little")
    return int.from_bytes(data[pe + 24 + 68:pe + 24 + 70], "little")


@check("Headless AC4/AC5/AC6: usage, a console program, the version marker, a hidden background engine")
def check_headless_cli():
    assert HEADLESS.is_file(), f"build the headless binary first: {HEADLESS}"
    assert windows_subsystem(HEADLESS) == 3, "the headless build isn't a console program"
    assert windows_subsystem(rc.EXE) == 2, "the full build isn't a GUI program"
    # A window would keep the process running past the timeout.
    for args in ([], ["--demo"], ["frobnicate"]):
        result = headless(*args)
        assert result.returncode == 2, (args, result.returncode)
        assert "engine" in result.stderr and "bridge" in result.stderr and "--version" in result.stderr, result.stderr
    version = rc.adeline("--version").stdout.strip()
    assert headless("--version").stdout.strip() == f"{version} (headless)", headless("--version").stdout
    assert "(headless)" not in version, version
    home = REMOTES.parent / "headless-home"
    home.mkdir(exist_ok=True)
    assert "not running" in headless("engine", "status", home=home).stdout
    started = headless("engine", "start", home=home)
    assert started.returncode == 0, started.stdout + started.stderr
    try:
        status = headless("engine", "status", home=home).stdout
        assert "Build: headless" in status, status
        pid = int(next(line for line in status.splitlines() if line.startswith("PID:")).split(":")[1])
        assert not [w for w in Desktop(backend="uia").windows(process=pid) if w.is_visible()], "the engine shows a window"
    finally:
        headless("engine", "stop", home=home)
    return f"engine {pid}"


@check("Headless AC3/AC11: a remote with the headless build connects, runs a prompt, reconnects")
def check_headless_remote():
    rc.stop_engine()
    make_remote("server")
    bin = REMOTES / "server" / ".adeline" / "bin"
    bin.mkdir(parents=True, exist_ok=True)
    (bin / "adeline.exe").write_bytes(HEADLESS.read_bytes())
    (bin / "version").write_text(rc.adeline("--version").stdout.strip())
    save_machines(("m-server", "Server", ["server"]))
    process, window = launch()
    try:
        wait(lambda: "app · Server" in tabs(window), 60, what="the headless remote's tab")
        tab(window, "app · Server").click_input()
        named(window, "New chat").click_input()
        send(window, "SLOW 4")
        conversation = wait(lambda: remote_conversations("server")[0], 20, what="a remote conversation")
        wait(lambda: remote_status("server")["conversations"], 20, what="the remote turn")
        assert remote_status("server")["headless"], "the remote engine isn't the headless build"
        for pid in bridges():
            rc.kill(pid)
        wait(lambda: containing(window, "Server is disconnected"), 10, what="the disconnected banner")
        wait(lambda: not containing(window, "Server is disconnected"), 30, what="the reconnect")
        wait(lambda: "turn_finished" in remote_transcript("server", conversation), 30, what="the turn")
        assert (bin / "adeline.exe").read_bytes() == HEADLESS.read_bytes(), "the headless build was replaced"
        # UI Automation can't read Settings' "Build" line; it shows this flag.
        local = rc.adeline("engine", "status").stdout
        assert "Build: full" in local, local
    finally:
        process.kill()


def main():
    global REMOTES
    temp = rc.setup()
    REMOTES = temp / "remotes"
    REMOTES.mkdir()
    os.environ["ADELINE_SSH"] = f"{sys.executable} {os.path.join(os.path.dirname(os.path.abspath(__file__)), 'fake_ssh.py')}"
    os.environ["ADELINE_FAKE_SSH_ROOT"] = str(REMOTES)
    # One agent, so new chats pick it without a menu.
    for folder in ("fake",):
        path = rc.config / "agents" / folder / "agent.yml"
        path.unlink()
        path.parent.rmdir()
    try:
        only = sys.argv[1] if len(sys.argv) > 1 else ""
        for run in (check_launch, check_quit_background, check_quit_idle, check_quit_stop_all,
                    check_settings_engine, check_unavailable, check_demo, check_demo_conversation, check_fork,
                    check_demo_machines, check_add_machine, check_drop, check_ssh_failures,
                    check_upgrade_and_browse, check_headless_cli, check_headless_remote):
            if only.lower() in run.label.lower():
                run()
    finally:
        rc.stop_engine()
        stop_remotes()
        print(f"Temp home: {temp}")
    failed = results.count(False)
    print(f"{len(results) - failed} passed, {failed} failed")
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
