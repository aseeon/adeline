"""A fake ACP v1 agent: newline-delimited JSON-RPC 2.0 on stdin/stdout.

Keywords in the prompt script each turn, applied in this order:
  FAIL <n>       fail the first n attempts with a temporary-looking error
  SLOW <secs>    stream one chunk every 0.5 s for that long
  PERMISSION     ask the client for an edit permission and wait for the answer
  HANG_ON_CLOSE  from now on ignore session/close and stdin EOF
Without keywords the turn streams a short greeting. Every wait honors
session/cancel and then replies stopReason "cancelled".

Environment:
  ADELINE_FAKE_PIDS   folder; the agent writes an empty file named after its PID
  ADELINE_FAKE_STATE  folder for FAIL attempt counters (default: next to this script)
"""
import itertools
import json
import os
import re
import sys
import threading
import time
import uuid
from pathlib import Path

MODEL_OPTION = {
    "id": "model",
    "name": "Model",
    "category": "model",
    "type": "select",
    "currentValue": "fake-model",
    "options": [{"value": "fake-model", "name": "Fake"}],
}

out_lock = threading.Lock()
request_ids = itertools.count(1_000_000)
replies = {}  # outgoing request id -> [threading.Event, response]
cancels = {}  # session id -> threading.Event of the running prompt
hang_on_close = threading.Event()


def send(message):
    message["jsonrpc"] = "2.0"
    with out_lock:
        sys.stdout.write(json.dumps(message) + "\n")
        sys.stdout.flush()


def log(text):
    print(f"fake_agent[{os.getpid()}]: {text}", file=sys.stderr, flush=True)


def chunk(session, text):
    send({
        "method": "session/update",
        "params": {
            "sessionId": session,
            "update": {"sessionUpdate": "agent_message_chunk", "content": {"type": "text", "text": text}},
        },
    })


def attempts_file(session):
    folder = Path(os.environ.get("ADELINE_FAKE_STATE") or Path(__file__).with_name(".fake-attempts"))
    folder.mkdir(parents=True, exist_ok=True)
    return folder / re.sub(r"[^\w.-]", "_", session)


def ask_permission(session, cancel):
    """Returns the chosen optionId, or None when cancelled."""
    request_id = next(request_ids)
    waiter = [threading.Event(), None]
    replies[request_id] = waiter
    send({
        "id": request_id,
        "method": "session/request_permission",
        "params": {
            "sessionId": session,
            "toolCall": {"toolCallId": f"tool-{request_id}", "title": "Edit fake.txt", "kind": "edit", "status": "pending"},
            "options": [
                {"optionId": "allow", "name": "Allow once", "kind": "allow_once"},
                {"optionId": "reject", "name": "Reject", "kind": "reject_once"},
            ],
        },
    })
    while not waiter[0].wait(0.05):
        if cancel.is_set():
            break
    replies.pop(request_id, None)
    outcome = ((waiter[1] or {}).get("result") or {}).get("outcome") or {}
    return outcome.get("optionId") if outcome.get("outcome") == "selected" else None


def prompt(message):
    """Runs one session/prompt on its own thread so cancel and answers keep flowing."""
    params = message["params"]
    session = params["sessionId"]
    text = " ".join(part.get("text", "") for part in params.get("prompt", []))
    cancel = cancels[session]

    def finish(reason):
        send({"id": message["id"], "result": {"stopReason": reason}})

    if "HANG_ON_CLOSE" in text:
        hang_on_close.set()
    if match := re.search(r"FAIL (\d+)", text):
        path = attempts_file(session)
        attempt = int(path.read_text() or 0) + 1 if path.exists() else 1
        path.write_text(str(attempt))
        if attempt <= int(match[1]):
            send({"id": message["id"], "error": {"code": -32000, "message": f"upstream timeout (attempt {attempt})"}})
            return
        path.unlink()
    if match := re.search(r"SLOW (\d+(?:\.\d+)?)", text):
        for index in range(max(1, round(float(match[1]) / 0.5))):
            if cancel.wait(0.5):
                return finish("cancelled")
            chunk(session, f"tick {index + 1} ")
    if "PERMISSION" in text:
        choice = ask_permission(session, cancel)
        if choice is None or cancel.is_set():
            return finish("cancelled")
        chunk(session, "Permission granted. " if choice == "allow" else "Permission rejected. ")
    for piece in ("Hello ", "from ", "fake agent"):
        if cancel.is_set():
            return finish("cancelled")
        chunk(session, piece)
        time.sleep(0.02)
    finish("end_turn")


def handle(message):
    method = message.get("method")
    if method is None:  # A response to one of our requests.
        if waiter := replies.get(message.get("id")):
            waiter[1] = message
            waiter[0].set()
        return
    params = message.get("params") or {}
    reply = lambda result: send({"id": message["id"], "result": result})
    match method:
        case "initialize":
            reply({
                "protocolVersion": 1,
                "agentCapabilities": {"loadSession": True, "sessionCapabilities": {"resume": {}, "close": {}}},
                "agentInfo": {"name": "fake", "version": "1.0"},
                "authMethods": [],
            })
        case "session/new":
            reply({"sessionId": str(uuid.uuid4()), "configOptions": [MODEL_OPTION]})
        case "session/resume" | "session/load":
            reply({"configOptions": [MODEL_OPTION]})
        case "session/set_config_option":
            option = dict(MODEL_OPTION)
            if params.get("configId") == "model":
                option["currentValue"] = params.get("value")
            reply({"configOptions": [option]})
        case "session/prompt":
            # Cleared here, in arrival order, so a cancel right after the prompt isn't lost.
            cancels[params["sessionId"]] = threading.Event()
            threading.Thread(target=prompt, args=(message,), daemon=True).start()
        case "session/cancel":
            cancels.setdefault(params.get("sessionId"), threading.Event()).set()
        case "session/close":
            if hang_on_close.is_set():
                log("ignoring session/close")
            else:
                reply({})
        case _ if "id" in message:
            send({"id": message["id"], "error": {"code": -32601, "message": f"Method not found: {method}"}})


def main():
    sys.stdin.reconfigure(encoding="utf-8")
    sys.stdout.reconfigure(encoding="utf-8", newline="\n")
    if folder := os.environ.get("ADELINE_FAKE_PIDS"):
        Path(folder).mkdir(parents=True, exist_ok=True)
        (Path(folder) / str(os.getpid())).touch()
    for line in sys.stdin:
        if line.strip():
            handle(json.loads(line))
    if hang_on_close.is_set():
        log("stdin closed; hanging until killed")
        while True:
            time.sleep(3600)


if __name__ == "__main__":
    main()
