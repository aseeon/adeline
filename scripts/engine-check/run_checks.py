"""End-to-end checks of the conversation engine (docs/scope-conversation-engine.md).

Runs target/release/adeline.exe against an isolated temporary home: every
process started here gets USERPROFILE and HOME pointing at it, so the real
engine and data are never touched. Prints PASS/FAIL per check; exits 1 on any failure.

    python scripts/engine-check/run_checks.py [--fast] [--only TEXT] [--keep]
"""
import argparse
import ctypes
import json
import os
import random
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time
import traceback
from concurrent.futures import ThreadPoolExecutor
from ctypes import wintypes
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
from engine_client import PROTOCOL, Client, EngineError, pipe_exists  # noqa: E402

EXE = HERE.parents[1] / "target" / "release" / "adeline.exe"
PROJECT = "demo"
STATUS_FIELDS = ["PID:", "Version:", "Protocol:", "Daemon mode:", "Uptime:", "Connected clients:", "Active conversations:"]

# Set by setup(): the temporary home and folders inside it.
home = config = work = pids = None


# ---------------------------------------------------------------------------
# Processes (Toolhelp snapshot: parent PID and thread count without psutil)

class PROCESSENTRY32W(ctypes.Structure):
    _fields_ = [("dwSize", wintypes.DWORD), ("cntUsage", wintypes.DWORD), ("th32ProcessID", wintypes.DWORD),
                ("th32DefaultHeapID", ctypes.c_size_t), ("th32ModuleID", wintypes.DWORD),
                ("cntThreads", wintypes.DWORD), ("th32ParentProcessID", wintypes.DWORD),
                ("pcPriClassBase", wintypes.LONG), ("dwFlags", wintypes.DWORD), ("szExeFile", wintypes.WCHAR * 260)]


_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_kernel32.CreateToolhelp32Snapshot.restype = wintypes.HANDLE


def processes():
    """{pid: (parent pid, thread count, exe name)} for every process."""
    snapshot = _kernel32.CreateToolhelp32Snapshot(2, 0)  # TH32CS_SNAPPROCESS
    entry = PROCESSENTRY32W(dwSize=ctypes.sizeof(PROCESSENTRY32W))
    result = {}
    try:
        more = _kernel32.Process32FirstW(snapshot, ctypes.byref(entry))
        while more:
            result[entry.th32ProcessID] = (entry.th32ParentProcessID, entry.cntThreads, entry.szExeFile)
            more = _kernel32.Process32NextW(snapshot, ctypes.byref(entry))
    finally:
        _kernel32.CloseHandle(wintypes.HANDLE(snapshot))
    return result


def alive(pid):
    return pid in processes()


def ancestors(pid):
    table, chain = processes(), []
    while pid in table and pid not in chain:
        chain.append(pid)
        pid = table[pid][0]
    return chain[1:]


def kill(pid):
    try:
        os.kill(pid, signal.SIGTERM)  # TerminateProcess on Windows.
    except OSError:
        pass


def agent_pids():
    return {int(path.name) for path in pids.iterdir()} if pids.exists() else set()


# ---------------------------------------------------------------------------
# Helpers

class Failed(AssertionError):
    pass


def expect(condition, message):
    if not condition:
        raise Failed(message)


def wait_until(predicate, timeout, interval=0.1):
    deadline = time.monotonic() + timeout
    while True:
        value = predicate()
        if value or time.monotonic() > deadline:
            return value
        time.sleep(interval)


def adeline(*args, timeout=60):
    return subprocess.run([str(EXE), *args], capture_output=True, text=True, timeout=timeout)


def running_pid():
    try:
        with Client(cli=True, timeout=2) as client:
            return client.call({"op": "status"}, 5)["pid"]
    except (OSError, EngineError, TimeoutError):
        return None


def start_engine(*flags):
    result = adeline("engine", "start", *flags)
    expect(result.returncode == 0, f"engine start exited {result.returncode}: {result.stdout}{result.stderr}")
    pid = wait_until(running_pid, 10)
    expect(pid, "engine start returned but no engine answers")
    return pid


def stop_engine():
    """Shuts the test engine down (killing it if needed) and kills leftover fake agents."""
    pid = running_pid()
    if pid:
        try:
            with Client(cli=True, timeout=2) as client:
                client.request({"op": "shutdown"}, 30)
        except (OSError, EngineError, TimeoutError):
            pass
        if not wait_until(lambda: not alive(pid), 15):
            kill(pid)
    for agent in agent_pids():
        kill(agent)
        (pids / str(agent)).unlink(missing_ok=True)


def send_command(prompt, conversation=None, agent="fake-auto"):
    return {"op": "send", "project_id": PROJECT, "conversation_id": conversation,
            "agent_id": None if conversation else agent, "permission_mode": None, "prompt": prompt}


def new_conversation(client, prompt, agent="fake-auto"):
    """Starts a conversation; returns (id, delta mark taken before the send)."""
    mark = client.mark()
    return client.call(send_command(prompt, agent=agent)), mark


def wait_turn(client, conversation, after, timeout=20):
    """Waits for a turn that started after `after` to stop processing; returns the final live state."""
    index, _ = client.wait_live(conversation, lambda live: live["processing"], timeout, after)
    return client.wait_live(conversation, lambda live: not live["processing"], timeout, index)[1]


def transcript(conversation, strict=False):
    path = config / "projects" / PROJECT / "conversations" / conversation / "transcript.jsonl"
    events = []
    for line in path.read_text(encoding="utf-8").splitlines() if path.exists() else []:
        try:
            events.append(json.loads(line))
        except ValueError:
            if strict:
                raise
    return events


def fresh_snapshot():
    with Client() as client:
        return client.snapshot


def samples(read, count, interval=0.5):
    values = []
    for _ in range(count):
        time.sleep(interval)
        values.append(read())
    return values


def lifecycle(events, name):
    return [e for e in events if e["kind"] == "lifecycle" and e["data"].get("event") == name]


def concurrently(*calls):
    """Runs the calls at the same moment; returns their results in order."""
    barrier = threading.Barrier(len(calls))

    def run(call):
        barrier.wait()
        return call()

    with ThreadPoolExecutor(len(calls)) as pool:
        return [future.result() for future in [pool.submit(run, call) for call in calls]]


# ---------------------------------------------------------------------------
# Setup

def write_agent(folder, name, mode):
    path = config / "agents" / folder
    path.mkdir(parents=True, exist_ok=True)
    text = (f"version: 1\nname: {json.dumps(name)}\nharness: custom\ncommand: {json.dumps(sys.executable)}\n"
            f"arguments: [{json.dumps(str(HERE / 'fake_agent.py'))}]\npermission_mode: {mode}\n")
    (path / "agent.yml.tmp").write_text(text, encoding="utf-8")
    os.replace(path / "agent.yml.tmp", path / "agent.yml")


def write_project(folder, name, directory):
    path = config / "projects" / folder
    (path / "conversations").mkdir(parents=True, exist_ok=True)
    (path / "project.yml.tmp").write_text(f"name: {json.dumps(name)}\ndirectory: {json.dumps(str(directory))}\n",
                                          encoding="utf-8")
    os.replace(path / "project.yml.tmp", path / "project.yml")


def setup():
    global home, config, work, pids
    temp = Path(tempfile.mkdtemp(prefix="adeline-engine-check-"))
    home, work, pids = temp / "home", temp / "work", temp / "pids"
    config = home / ".config" / "adeline"
    for folder in (work, pids, config):
        folder.mkdir(parents=True)
    # Inherited by every adeline.exe started here, and by the agents the engine starts.
    os.environ.update(USERPROFILE=str(home), HOME=str(home), ADELINE_FAKE_PIDS=str(pids),
                      ADELINE_FAKE_STATE=str(temp / "fake-state"))
    (config / "settings.yml").write_text("modes:\n  chats:\n    retry_limit: 3\n", encoding="utf-8")
    write_agent("fake", "Fake", "Ask")
    write_agent("fake-auto", "Fake Auto", "AllowEverything")
    write_project(PROJECT, PROJECT, work)
    return temp


# ---------------------------------------------------------------------------
# Checks. Each starts and ends with no engine running; a returned string is reported.

CHECKS = []


def check(name, slow=False):
    def register(function):
        CHECKS.append((name, function, slow))
        return function
    return register


@check("AC21/AC18/AC19 log file, pipe, same-user connection, protocol mismatch")
def check_basics():
    pid = start_engine()
    log = config / "engine" / "logs" / "engine.log"
    expect(log.is_file() and log.stat().st_size > 0, f"{log} is missing or empty")
    expect(pipe_exists(), "engine pipe not found under \\\\.\\pipe\\")
    with Client() as client:
        expect(client.snapshot["status"]["pid"] == pid, "snapshot status has another PID")
        expect(client.welcome["protocol"] == PROTOCOL, f"welcome protocol {client.welcome['protocol']}")
    with Client(protocol=999) as old:
        expect(old.snapshot is None, "a mismatched client received a snapshot")
        expect("Err" in old.request(send_command("hi")), "a mismatched client could send")
        expect("Ok" in old.request({"op": "status"}), "status must work across versions")
    files = sorted(p.name for p in (config / "engine").iterdir())
    return f"engine/ holds {files}"


@check("AC3 engine status/start/stop CLI")
def check_cli():
    result = adeline("engine", "status")
    expect(result.returncode == 1, f"status without engine exited {result.returncode}")
    result = adeline("engine", "start")
    match = re.search(r"PID (\d+)", result.stdout)
    expect(result.returncode == 0 and match, f"start: {result.returncode} {result.stdout!r}")
    pid = int(match[1])
    expect(alive(pid), f"started PID {pid} is not running")
    result = adeline("engine", "start")
    expect(result.returncode == 0 and "already running" in result.stdout, f"second start: {result.stdout!r}")
    expect(running_pid() == pid, "second start replaced the engine")
    result = adeline("engine", "status")
    expect(result.returncode == 0, f"status exited {result.returncode}")
    missing = [field for field in STATUS_FIELDS if field not in result.stdout]
    expect(not missing and f"PID: {pid}" in result.stdout and "Daemon mode: off" in result.stdout,
           f"status output lacks {missing}: {result.stdout!r}")
    result = adeline("engine", "stop")
    still_alive = alive(pid)
    expect(result.returncode == 0 and "stopped" in result.stdout.lower(), f"stop: {result.stdout!r}")
    expect(not still_alive, f"engine {pid} was still running when `engine stop` returned")
    start_engine("--daemon")
    result = adeline("engine", "status")
    expect("Daemon mode: on" in result.stdout, f"--daemon not reported: {result.stdout!r}")


@check("AC2 simultaneous starts end on one engine")
def check_single_engine():
    logs = [open(work / f"engine-{i}.out", "w+") for i in range(5)]
    procs = [subprocess.Popen([str(EXE), "engine"], stdout=log, stderr=subprocess.STDOUT) for log in logs]
    wait_until(lambda: sum(p.poll() is None for p in procs) <= 1, 15)
    survivors = [p for p in procs if p.poll() is None]
    losers = [p for p in procs if p.poll() is not None]
    expect(len(survivors) == 1, f"{len(survivors)} engines still running")
    expect(all(p.returncode == 0 for p in losers), f"loser exit codes {[p.returncode for p in losers]}")
    noisy = []
    for log, process in zip(logs, procs):
        log.seek(0)
        if process in losers and (text := log.read()):
            noisy.append(text)
    expect(not noisy, f"losing engines printed {noisy}")
    expect(wait_until(running_pid, 10) == survivors[0].pid, "clients don't reach the surviving engine")
    with Client() as a, Client() as b:
        expect(a.call({"op": "status"})["clients"] == 2, "two clients are not both connected")
    stop_engine()
    for log in logs:
        log.close()
    # `engine start` racing itself: every caller reports the same engine.
    with ThreadPoolExecutor(3) as pool:
        results = list(pool.map(lambda _: adeline("engine", "start"), range(3)))
    reported = {m[1] for r in results if (m := re.search(r"PID (\d+)", r.stdout))}
    expect(all(r.returncode == 0 for r in results) and len(reported) == 1,
           f"racing starts: {[(r.returncode, r.stdout.strip()) for r in results]}")


@check("AC1/AC22 agents are engine children and die with it; interrupted turn retries")
def check_containment():
    pid = start_engine()
    before = agent_pids()
    client = Client()
    conversation = client.call(send_command("SLOW 30"))
    agent = wait_until(lambda: agent_pids() - before, 15)
    expect(len(agent or ()) == 1, f"expected one new agent process, got {agent}")
    agent = agent.pop()
    expect(pid in ancestors(agent), f"agent {agent} ancestors {ancestors(agent)} lack engine {pid}")
    kill(pid)
    expect(wait_until(lambda: not alive(agent), 5), f"agent {agent} survived the killed engine")
    expect(wait_until(lambda: client.closed, 5) and not client.bye, "client saw a goodbye from a killed engine")
    client.close()
    start_engine()
    with Client() as client:
        error = client.snapshot["live"][conversation].get("error") or ""
        expect("interrupted" in error.lower(), f"restarted engine shows {error!r}, not interrupted")
        mark = client.mark()
        client.call({"op": "retry", "id": conversation})
        live = wait_turn(client, conversation, mark)
        expect(live["error"] is None, f"retry after crash failed: {live['error']}")


@check("AC10 two clients: shared changes, one of two simultaneous sends wins")
def check_concurrent_send():
    start_engine()
    with Client() as a, Client() as b:
        conversation, mark = new_conversation(a, "hello")
        wait_turn(a, conversation, mark)
        b.wait_delta(lambda d: d.get("delta") == "thread" and d["thread"]["id"] == conversation, 2)
        mark_a, mark_b = a.mark(), b.mark()
        results = concurrently(*[lambda c=c: c.request(send_command("SLOW 2", conversation)) for c in (a, b)])
        errors = [r["Err"] for r in results if "Err" in r]
        expect(len(errors) == 1 and "already processing" in errors[0], f"simultaneous sends gave {results}")
        is_text = lambda d: d.get("delta") == "text" and d["id"] == conversation
        a.wait_delta(is_text, 10, mark_a)
        b.wait_delta(is_text, 10, mark_b)
        wait_turn(a, conversation, mark_a)


@check("AC11 permission race, repeated Stop, archive racing send")
def check_conflicts():
    start_engine()
    with Client() as a, Client() as b:
        conversation, mark = new_conversation(a, "PERMISSION", agent="fake")
        mark_b = b.mark()
        _, live = a.wait_live(conversation, lambda l: l["permission"], 15, mark)
        b.wait_live(conversation, lambda l: l["permission"], 5, mark_b)
        request = live["permission"][0]
        allow = next(o["option_id"] for o in request["options"] if o["kind"] == "allow_once")
        answer = {"op": "answer_permission", "id": conversation, "request_id": request["request_id"],
                  "option_id": allow}
        results = concurrently(lambda: a.request(answer), lambda: b.request(answer))
        expect(sorted(results, key=str) == [{"Err": "Permission already answered"}, {"Ok": None}],
               f"double answer gave {results}")
        a.wait_live(conversation, lambda l: not l["processing"], 15, mark)

        conversation, mark = new_conversation(a, "SLOW 10")
        a.wait_live(conversation, lambda l: l["processing"], 10, mark)
        stop = {"op": "stop", "id": conversation}
        results = concurrently(*[lambda c=c: c.request(stop) for c in (a, b, a, b)])
        expect(all(r == {"Ok": None} for r in results), f"repeated Stop gave {results}")
        a.wait_live(conversation, lambda l: not l["processing"], 5, mark)
        expect(a.request(stop) == {"Ok": None}, "Stop on an idle conversation failed")

        mark_a, mark_b = a.mark(), b.mark()
        results = concurrently(lambda: a.request({"op": "set_status", "id": conversation, "status": "archived"}),
                               lambda: b.request(send_command("hi", conversation)))
        expect(all("Ok" in r or r["Err"] for r in results), f"rejection without a reason: {results}")
        for client, after in ((a, mark_a), (b, mark_b)):
            client.wait_delta(lambda d: d.get("delta") == "status" and d["id"] == conversation
                              and d["status"] == "archived", 5, after)
        def settled():
            snapshot = fresh_snapshot()
            return None if snapshot["live"][conversation]["processing"] else snapshot
        snapshot = wait_until(settled, 10, 0.5)
        expect(snapshot, "archived conversation still processing")
        status = next(t["status"] for p in snapshot["projects"] for t in p["threads"] if t["id"] == conversation)
        expect(status == "archived", f"after archive racing send, status is {status}")
        reopen = {"op": "set_status", "id": conversation, "status": "idle"}
        expect(a.request(reopen) == {"Ok": None}, "reopening failed")
        expect(b.request(reopen) == {"Err": "Already open"}, "second reopen was not rejected as Already open")
        return f"archive vs send: {results}"


@check("AC11 stress: conflicting commands from 3 clients for 20 s")
def check_stress():
    start_engine()
    setup_client = Client()
    conversations = []
    for agent in ("fake-auto", "fake-auto", "fake"):
        conversation, mark = new_conversation(setup_client, "hi", agent)
        wait_turn(setup_client, conversation, mark)
        conversations.append(conversation)
    setup_client.close()
    stats = {"requests": 0, "ok": 0, "err": 0, "status_max": 0.0}
    problems, end = [], time.monotonic() + 20

    def command(client, rng):
        conversation = rng.choice(conversations)
        pending = [d["live"]["permission"] for d in client.deltas[-50:]
                   if d.get("delta") == "live" and d["id"] == conversation and d["live"]["permission"]]
        request_id = pending[-1][0]["request_id"] if pending else rng.randint(1, 5)
        return rng.choice([
            send_command(rng.choice(["hi", "SLOW 1", "PERMISSION", "FAIL 1", "SLOW 0.5 PERMISSION"]), conversation),
            {"op": "stop", "id": conversation},
            {"op": "retry", "id": conversation},
            {"op": "set_status", "id": conversation, "status": rng.choice(["archived", "idle", "completed"])},
            {"op": "answer_permission", "id": conversation, "request_id": request_id,
             "option_id": rng.choice(["allow", "reject"])},
            {"op": "mark_read", "id": conversation, "through": rng.randint(0, 5)},
        ])

    def worker(seed):
        rng = random.Random(seed)
        with Client() as client:
            while time.monotonic() < end and not problems:
                cmd = command(client, rng)
                try:
                    result = client.request(cmd, 10)
                except (TimeoutError, EngineError) as error:
                    problems.append(f"{cmd['op']}: {error}")
                    return
                stats["requests"] += 1
                stats["ok" if "Ok" in result else "err"] += 1
                time.sleep(rng.random() * 0.05)

    def monitor():
        with Client(cli=True) as client:
            while time.monotonic() < end and not problems:
                started = time.monotonic()
                try:
                    client.call({"op": "status"}, 2)
                except (TimeoutError, EngineError) as error:
                    problems.append(f"status: {error}")
                    return
                stats["status_max"] = max(stats["status_max"], time.monotonic() - started)
                time.sleep(0.5)

    threads = [threading.Thread(target=worker, args=(seed,)) for seed in range(3)]
    threads.append(threading.Thread(target=monitor))
    for thread in threads:
        thread.start()
    for thread in threads:
        thread.join()
    expect(not problems, f"engine stopped answering: {problems}")
    started = time.monotonic()
    stop_engine()
    expect(time.monotonic() - started < 25, "shutdown after the stress test took too long")
    broken = []
    for path in (config / "projects").rglob("transcript.jsonl"):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            try:
                json.loads(line)
            except ValueError:
                broken.append(f"{path.parent.name}:{number}")
    expect(not broken, f"invalid transcript lines: {broken}")
    return (f"{stats['requests']} requests ({stats['ok']} ok, {stats['err']} rejected), "
            f"slowest status {stats['status_max']:.2f} s")


@check("AC8 permission needed with no client: turn stops, Retry continues")
def check_permission_without_client():
    start_engine()
    for prompt, wait_for_request in (("PERMISSION", True), ("SLOW 2 PERMISSION", False)):
        client = Client()
        conversation, mark = new_conversation(client, prompt, agent="fake")
        if wait_for_request:  # Waiting when the last client leaves.
            client.wait_live(conversation, lambda l: l["permission"], 15, mark)
        client.close()  # Otherwise the request arrives after it left.

        def stopped():
            events = transcript(conversation)
            return lifecycle(events, "turn_cancelled") and [
                e for e in events if e["kind"] == "error" and e["data"].get("permission")]
        events = wait_until(stopped, 15)
        expect(events, f"{prompt!r}: turn did not stop for the missing permission")
        expect("waiting for permission" in events[0]["data"]["message"].lower(),
               f"{prompt!r}: transcript error {events[0]['data']['message']!r}")
        with Client() as client:
            live = client.snapshot["live"][conversation]
            expect(not live["processing"] and "waiting for permission" in (live["error"] or "").lower(),
                   f"{prompt!r}: reconnected client sees {live}")
            mark = client.mark()
            client.call({"op": "retry", "id": conversation})
            live = wait_turn(client, conversation, mark)
            expect(live["error"] is None, f"{prompt!r}: retry failed: {live['error']}")


@check("AC7/AC4 Finish in background: turns and retries complete, engine exits after 60 s", slow=True)
def check_background():
    pid = start_engine()
    client = Client()
    expect(client.snapshot["settings"]["retry_limit"] >= 2, "retry limit too low for FAIL 2")
    before = agent_pids()
    idle, mark = new_conversation(client, "hi")
    wait_turn(client, idle, mark)
    idle_agent = agent_pids() - before
    before = agent_pids()
    slow = client.call(send_command("SLOW 8"))
    retrying = client.call(send_command("FAIL 2"))
    client.close()
    expect(wait_until(lambda: not any(alive(p) for p in idle_agent), 10), "idle agent kept running after quit")
    finished = lambda: all(lifecycle(transcript(c), "turn_finished") for c in (slow, retrying))
    expect(wait_until(finished, 30), "background turns did not finish")
    done = time.monotonic()
    expect([e for e in transcript(slow) if e["kind"] == "assistant_chunk"], "no reply text in the transcript")
    retries = [e for e in transcript(retrying) if e["kind"] == "error" and "attempt" in e["data"]]
    expect(len(retries) == 2, f"expected 2 recorded retries, got {len(retries)}")
    expect(wait_until(lambda: not any(alive(p) for p in agent_pids() - before), 10),
           "agents of finished background turns kept running")
    expect(wait_until(lambda: not alive(pid), 90, 0.5), "engine did not exit on its own")
    waited = time.monotonic() - done
    expect(55 <= waited <= 75, f"engine exited {waited:.0f} s after the work finished, expected about 60")
    return f"engine exited {waited:.0f} s after the last turn"


@check("AC6 Stop all kills a hung agent after 5 s and replies once all exited")
def check_stop_all():
    pid = start_engine()
    with Client() as client:
        before = agent_pids()
        hung, mark = new_conversation(client, "HANG_ON_CLOSE")
        wait_turn(client, hung, mark)
        client.call(send_command("SLOW 30"))
        agents = wait_until(lambda: len(agent_pids() - before) == 2 and agent_pids() - before, 15)
        expect(agents, "expected two agent processes")
        started = time.monotonic()
        result = client.call({"op": "stop_all"}, 30)
        took = time.monotonic() - started
        survivors = [p for p in agents if alive(p)]
        expect(not survivors, f"agents {survivors} still running when stop_all replied")
        expect(4 <= took <= 9, f"stop_all took {took:.1f} s, expected 5 s grace plus the kill")
        expect(len(result["stopped"]) == 2, f"stopped list {result['stopped']}")
        expect(alive(pid), "stop_all must not stop the engine")
        return f"stop_all replied after {took:.1f} s: {result['stopped']}"


@check("AC14 retry limit migrates and a change applies to a running turn")
def check_retry_limit():
    start_engine()
    with Client() as client:
        migrated = client.snapshot["settings"]["retry_limit"]
        expect(migrated == 3, f"retry limit {migrated}, expected 3 copied from settings.yml")
        client.call({"op": "set_settings", "settings": {"keep_running": False, "retry_limit": 1}})
        conversation, mark = new_conversation(client, "FAIL 3")
        client.wait_live(conversation, lambda l: (l["progress"] or "").startswith("Retry 1 of 1"), 10, mark)
        client.call({"op": "set_settings", "settings": {"keep_running": False, "retry_limit": 5}})
        live = wait_turn(client, conversation, mark, 30)
        expect(live["error"] is None, f"turn failed under the old limit: {live['error']}")
    saved = (config / "engine" / "settings.yml").read_text(encoding="utf-8")
    expect("retry_limit: 5" in saved, f"engine/settings.yml: {saved!r}")


@check("AC15 agent and project files edited on disk reach clients")
def check_file_watching():
    start_engine()
    with Client() as client:
        def agent_delta(test, after):
            return client.wait_delta(lambda d: d.get("delta") == "agents" and test(
                {e["id"]: e["definition"] for e in d["entries"]}), 10, after)

        mark = client.mark()
        write_agent("watched", "Watched", "Ask")
        agent_delta(lambda agents: "watched" in agents, mark)
        mark = client.mark()
        write_agent("watched", "Watched", "AllowEverything")
        agent_delta(lambda agents: agents.get("watched", {}).get("permission_mode") == "AllowEverything", mark)
        mark = client.mark()
        shutil.rmtree(config / "agents" / "watched")
        agent_delta(lambda agents: "watched" not in agents, mark)

        def project_delta(test, after):
            return client.wait_delta(lambda d: d.get("delta") == "project" and test(d["workspace"]["config"]),
                                     10, after)

        other = work / "other"
        other.mkdir(exist_ok=True)
        mark = client.mark()
        write_project("other", "Other", work)
        project_delta(lambda c: c["id"] == "other", mark)
        mark = client.mark()
        write_project("other", "Other", other)
        project_delta(lambda c: c["id"] == "other" and Path(c["directory"]) == other, mark)
        mark = client.mark()
        shutil.rmtree(config / "projects" / "other")
        client.wait_delta(lambda d: d.get("delta") == "project_removed" and d["id"] == "other", 10, mark)


@check("Fork AC2/AC4/AC6-AC9 native fork at the latest reply, text copy otherwise")
def check_fork():
    start_engine()

    def sent(conversation, method):
        return [e["data"]["message"] for e in transcript(conversation)
                if e["kind"] == "raw" and e["data"]["direction"] == "outgoing"
                and e["data"]["message"].get("method") == method]

    def text_copy(conversation):
        return any(e["kind"] == "fork_text_copy" for e in transcript(conversation))

    def send_in(client, conversation, prompt):
        mark = client.mark()
        client.call(send_command(prompt, conversation))
        return wait_turn(client, conversation, mark)

    with Client() as client:
        source, mark = new_conversation(client, "first")
        wait_turn(client, source, mark)
        send_in(client, source, "second")
        fork = client.call({"op": "fork", "id": source, "message": 3})
        kinds = {e["kind"] for e in transcript(fork)}
        expect(not kinds & {"raw", "lifecycle", "session", "permission_decision"}, f"fork copied {kinds}")
        settings = (config / "projects" / PROJECT / "conversations" / fork / "conversation.yml").read_text()
        expect("(fork)" in settings and "session_id: null" in settings, settings)
        expect(not sent(fork, "initialize"), "forking started an agent")
        # AC7: the latest reply of an idle source forks natively.
        send_in(client, fork, "third")
        expect(sent(fork, "session/fork") and not text_copy(fork), "latest reply did not fork natively")
        expect(sent(fork, "session/prompt")[0]["params"]["prompt"][0]["text"] == "third",
               "native fork prompt carried the text copy")
        # AC8: an earlier reply gets the text copy and the note.
        early = client.call({"op": "fork", "id": source, "message": 1})
        send_in(client, early, "again")
        prompt = sent(early, "session/prompt")[0]["params"]["prompt"][0]["text"]
        expect(not sent(early, "session/fork") and text_copy(early), "earlier reply used a native fork")
        expect("user: first" in prompt and "second" not in prompt and prompt.endswith("again"), prompt)
        # AC9: a failed native fork falls back to the text copy.
        broken, mark = new_conversation(client, "NO_FORK")
        wait_turn(client, broken, mark)
        retry = client.call({"op": "fork", "id": broken, "message": 1})
        send_in(client, retry, "fallback")
        expect(sent(retry, "session/fork") and sent(retry, "session/new") and text_copy(retry),
               "failed native fork did not fall back")
        # AC6: forking a processing source leaves its turn running.
        mark = client.mark()
        client.call(send_command("SLOW 2", source))
        client.wait_live(source, lambda live: live["processing"], 10, mark)
        client.call({"op": "fork", "id": source, "message": 3})
        final = wait_turn(client, source, mark)
        expect(not final["error"], f"source turn ended with {final['error']}")
        expect(lifecycle(transcript(source), "turn_finished")[-1:], "source turn did not finish")


@check("AC20 thread count with 20 conversations, 5 processing")
def check_threads():
    pid = start_engine()
    count = lambda: processes()[pid][1]
    with Client() as client:
        first, mark = new_conversation(client, "hi")
        wait_turn(client, first, mark)
        time.sleep(3)
        base = min(samples(count, 3))
        for _ in range(14):
            conversation, mark = new_conversation(client, "hi")
            wait_turn(client, conversation, mark)
        for _ in range(5):
            client.call(send_command("SLOW 25"))
        expect(wait_until(lambda: sum(alive(p) for p in agent_pids()) >= 20, 20), "20 agents did not start")
        time.sleep(3)
        busy = samples(count, 6)
        report = f"threads: 1 conversation {base}, 20 conversations min {min(busy)} max {max(busy)}"
        expect(min(busy) - base <= 12, report)
        return report


# ---------------------------------------------------------------------------

def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--fast", action="store_true", help="skip the slow checks (about 90 s each)")
    parser.add_argument("--only", help="run only checks whose name contains this text")
    parser.add_argument("--keep", action="store_true", help="keep the temporary home even when all checks pass")
    args = parser.parse_args()
    if not EXE.is_file():
        sys.exit(f"{EXE} is missing; build it with `cargo build --release --locked`.")
    temp = setup()
    print(f"Temporary home: {home}")
    try:  # A build without the engine subcommand would open the UI instead.
        adeline("engine", "status", timeout=15)
    except subprocess.TimeoutExpired:
        shutil.rmtree(temp, ignore_errors=True)
        sys.exit(f"`{EXE.name} engine status` did not return; this build lacks the engine subcommand.")
    failures = 0
    for name, function, slow in CHECKS:
        if (args.fast and slow) or (args.only and args.only.lower() not in name.lower()):
            continue
        started = time.monotonic()
        try:
            stop_engine()
            note = function()
            verdict = "PASS"
        except Failed as error:
            verdict, note = "FAIL", str(error)
        except Exception:  # noqa: BLE001 - report and keep going
            verdict, note = "FAIL", traceback.format_exc().strip()
        finally:
            stop_engine()
        failures += verdict == "FAIL"
        print(f"{verdict}  {name}  ({time.monotonic() - started:.1f} s)" + (f"\n      {note}" if note else ""),
              flush=True)
    if failures or args.keep:
        print(f"Kept {temp}")
    else:
        shutil.rmtree(temp, ignore_errors=True)
    print(f"{failures} check(s) failed" if failures else "All checks passed")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
