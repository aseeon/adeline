"""Drives fake_agent.py over pipes the way Adeline's ACP worker does."""
import json
import os
import queue
import subprocess
import sys
import tempfile
import threading
import time
from pathlib import Path

AGENT = Path(__file__).with_name("fake_agent.py")


class Agent:
    def __init__(self, env):
        self.process = subprocess.Popen(
            [sys.executable, str(AGENT)], stdin=subprocess.PIPE, stdout=subprocess.PIPE,
            text=True, encoding="utf-8", env=env,
        )
        self.lines = queue.Queue()
        threading.Thread(target=self._read, daemon=True).start()
        self.next_id = 0

    def _read(self):
        for line in self.process.stdout:
            self.lines.put(json.loads(line))

    def write(self, message):
        self.process.stdin.write(json.dumps({"jsonrpc": "2.0", **message}) + "\n")
        self.process.stdin.flush()

    def request(self, method, params):
        self.next_id += 1
        self.write({"id": self.next_id, "method": method, "params": params})
        return self.next_id

    def until_reply(self, request_id, timeout=10):
        """Returns (reply, every message seen before it)."""
        seen, deadline = [], time.monotonic() + timeout
        while True:
            message = self.lines.get(timeout=max(0.01, deadline - time.monotonic()))
            if message.get("id") == request_id and "method" not in message:
                return message, seen
            seen.append(message)

    def call(self, method, params):
        reply, _ = self.until_reply(self.request(method, params))
        assert "result" in reply, reply
        return reply["result"]

    def prompt(self, session, text):
        return self.request("session/prompt", {"sessionId": session, "prompt": [{"type": "text", "text": text}]})


def text_of(messages):
    return "".join(m["params"]["update"]["content"]["text"] for m in messages if m.get("method") == "session/update")


def main():
    temp = Path(tempfile.mkdtemp(prefix="fake-agent-test-"))
    env = dict(os.environ, ADELINE_FAKE_PIDS=str(temp / "pids"), ADELINE_FAKE_STATE=str(temp / "state"))
    agent = Agent(env)

    init = agent.call("initialize", {"protocolVersion": 1, "clientCapabilities": {}})
    assert init["protocolVersion"] == 1 and init["agentCapabilities"]["loadSession"] is True
    assert init["agentCapabilities"]["sessionCapabilities"] == {"resume": {}, "close": {}, "fork": {}}
    session = agent.call("session/new", {"cwd": str(temp), "mcpServers": []})
    sid = session["sessionId"]
    assert session["configOptions"][0]["currentValue"] == "fake-model"
    assert agent.call("session/set_config_option", {"sessionId": sid, "configId": "model", "value": "fake-model"})
    assert agent.call("session/resume", {"sessionId": sid, "cwd": str(temp), "mcpServers": []})["configOptions"]
    assert (temp / "pids" / str(agent.process.pid)).exists()

    reply, seen = agent.until_reply(agent.prompt(sid, "hi"))
    assert reply["result"] == {"stopReason": "end_turn"} and text_of(seen) == "Hello from fake agent", (reply, seen)

    started = time.monotonic()
    request = agent.prompt(sid, "SLOW 10")
    time.sleep(1.2)
    agent.write({"method": "session/cancel", "params": {"sessionId": sid}})
    reply, seen = agent.until_reply(request)
    assert reply["result"] == {"stopReason": "cancelled"} and time.monotonic() - started < 2.5, reply
    assert "tick 1" in text_of(seen)

    for option, expected in (("allow", "Permission granted"), ("reject", "Permission rejected")):
        request = agent.prompt(sid, "PERMISSION")
        asked = agent.lines.get(timeout=5)
        assert asked["method"] == "session/request_permission", asked
        assert asked["params"]["toolCall"]["kind"] == "edit"
        assert {o["kind"] for o in asked["params"]["options"]} == {"allow_once", "reject_once"}
        agent.write({"id": asked["id"], "result": {"outcome": {"outcome": "selected", "optionId": option}}})
        reply, seen = agent.until_reply(request)
        assert reply["result"]["stopReason"] == "end_turn" and expected in text_of(seen), (reply, seen)

    request = agent.prompt(sid, "PERMISSION")
    asked = agent.lines.get(timeout=5)
    agent.write({"id": asked["id"], "result": {"outcome": {"outcome": "cancelled"}}})
    assert agent.until_reply(request)[0]["result"] == {"stopReason": "cancelled"}

    request = agent.prompt(sid, "PERMISSION")
    agent.lines.get(timeout=5)
    agent.write({"method": "session/cancel", "params": {"sessionId": sid}})
    assert agent.until_reply(request)[0]["result"] == {"stopReason": "cancelled"}

    for attempt in (1, 2):
        reply, _ = agent.until_reply(agent.prompt(sid, "FAIL 2"))
        assert "upstream timeout" in reply["error"]["message"], reply
    agent.process.kill()  # The counter survives a restart.
    agent = Agent(env)
    agent.call("initialize", {"protocolVersion": 1})
    agent.call("session/load", {"sessionId": sid, "cwd": str(temp), "mcpServers": []})
    assert agent.until_reply(agent.prompt(sid, "FAIL 2"))[0]["result"] == {"stopReason": "end_turn"}

    assert agent.call("session/close", {"sessionId": sid}) == {}
    agent.process.stdin.close()
    assert agent.process.wait(5) == 0

    agent = Agent(env)
    agent.call("initialize", {"protocolVersion": 1})
    sid = agent.call("session/new", {"cwd": str(temp), "mcpServers": []})["sessionId"]
    agent.until_reply(agent.prompt(sid, "HANG_ON_CLOSE"))
    agent.request("session/close", {"sessionId": sid})
    agent.process.stdin.close()
    try:
        agent.process.wait(1.5)
        raise AssertionError("HANG_ON_CLOSE agent exited")
    except subprocess.TimeoutExpired:
        agent.process.kill()
    print("fake agent: all checks passed")


if __name__ == "__main__":
    main()
