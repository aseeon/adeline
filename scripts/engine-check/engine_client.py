"""A small client for the conversation engine protocol (src/protocol.rs).

One JSON object per line over the named pipe \\\\.\\pipe\\adeline-engine-<hash>.
The engine reads USERPROFILE, so set it before connecting to reach a test engine.
"""
import ctypes
import itertools
import json
import msvcrt
import os
import threading
import time
from ctypes import wintypes

PROTOCOL = 2
_kernel32 = ctypes.WinDLL("kernel32", use_last_error=True)
_kernel32.PeekNamedPipe.argtypes = [wintypes.HANDLE, ctypes.c_void_p, wintypes.DWORD,
                                    ctypes.c_void_p, ctypes.POINTER(wintypes.DWORD), ctypes.c_void_p]


def config_dir(userprofile=None):
    """`PathBuf::from(USERPROFILE).join(".config/adeline")` on Windows, as a string."""
    base = os.environ["USERPROFILE"] if userprofile is None else userprofile
    # Path::join adds a separator unless the base is empty, ends in one, or is a bare drive ("C:").
    if not base or base[-1] in "\\/" or (len(base) == 2 and base[1] == ":"):
        return base + ".config/adeline"
    return base + "\\.config/adeline"


def fnv1a64(data: bytes) -> int:
    value = 0xCBF29CE484222325
    for byte in data:
        value = ((value ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return value


def pipe_name(userprofile=None):
    """Mirrors `ipc::pipe_name`: FNV-1a 64 over the lowercased config path's UTF-8 bytes."""
    return rf"\\.\pipe\adeline-engine-{fnv1a64(config_dir(userprofile).lower().encode()):016x}"


def pipe_exists(userprofile=None):
    return pipe_name(userprofile).rsplit("\\", 1)[1] in os.listdir("\\\\.\\pipe\\")


class EngineError(Exception):
    pass


class Client:
    """A connection to the engine. A reader thread polls with PeekNamedPipe, so a
    read never blocks the synchronous pipe handle while another thread writes."""

    def __init__(self, hello=True, cli=False, protocol=PROTOCOL, timeout=10):
        deadline = time.monotonic() + timeout
        while True:
            try:
                self._pipe = open(pipe_name(), "r+b", buffering=0)
                break
            except OSError as error:  # 231 = ERROR_PIPE_BUSY: the next instance isn't ready yet.
                if getattr(error, "winerror", None) != 231 or time.monotonic() > deadline:
                    raise
                time.sleep(0.02)
        self._handle = msvcrt.get_osfhandle(self._pipe.fileno())
        self._ids = itertools.count(1)
        self._write_lock = threading.Lock()
        self._changed = threading.Condition()
        self.replies = {}
        self.deltas = []
        self.welcome = None
        self.snapshot = None
        self.bye = False
        self.closed = False
        threading.Thread(target=self._read, daemon=True).start()
        if hello:
            self.send({"type": "hello", "protocol": protocol, "cli": cli})
            subscribed = protocol == PROTOCOL and not cli
            self._wait(lambda: self.welcome and (self.snapshot or not subscribed), timeout, "welcome")

    # -- transport ---------------------------------------------------------

    def _read(self):
        buffer = b""
        available = wintypes.DWORD()
        try:
            while True:
                if not _kernel32.PeekNamedPipe(self._handle, None, 0, None, ctypes.byref(available), None):
                    break  # Broken pipe: the engine went away or we closed.
                if not available.value:
                    time.sleep(0.005)
                    continue
                buffer += self._pipe.read(available.value)
                *lines, buffer = buffer.split(b"\n")
                for line in lines:
                    if line.strip():
                        self._dispatch(json.loads(line))
        except (OSError, ValueError):
            pass
        with self._changed:
            self.closed = True
            self._changed.notify_all()

    def _dispatch(self, message):
        with self._changed:
            kind = message.get("type")
            if kind == "welcome":
                self.welcome = message["status"]
            elif kind == "snapshot":
                self.snapshot = message
            elif kind == "delta":
                self.deltas.append(message)
            elif kind == "reply":
                self.replies[message["id"]] = message["result"]
            elif kind == "bye":
                self.bye = True
            self._changed.notify_all()

    def _wait(self, ready, timeout, what):
        """Waits until `ready()` returns something truthy, and returns it."""
        result = None

        def check():
            nonlocal result
            result = ready()
            return result or self.closed

        with self._changed:
            if not self._changed.wait_for(check, timeout):
                raise TimeoutError(f"no {what} within {timeout} s")
        if not result:
            raise EngineError(f"connection closed while waiting for {what}")
        return result

    def send(self, message):
        with self._write_lock:
            self._pipe.write((json.dumps(message) + "\n").encode())

    def close(self):
        try:
            self._pipe.close()
        except OSError:
            pass

    def __enter__(self):
        return self

    def __exit__(self, *_):
        self.close()

    # -- requests ----------------------------------------------------------

    def request(self, command, timeout=15):
        """Sends a command such as {"op": "status"}; returns {"Ok": value} or {"Err": reason}."""
        request_id = next(self._ids)
        self.send({"type": "request", "id": request_id, "command": command})
        return self._wait(lambda: self.replies.pop(request_id, None), timeout, f"reply to {command['op']}")

    def call(self, command, timeout=15):
        """Like `request`, but returns the Ok value and raises EngineError on Err."""
        result = self.request(command, timeout)
        if "Err" in result:
            raise EngineError(result["Err"])
        return result["Ok"]

    # -- deltas ------------------------------------------------------------

    def mark(self):
        """The current position in `deltas`, for waiting only on later ones."""
        with self._changed:
            return len(self.deltas)

    def wait_delta(self, predicate, timeout=10, after=0):
        """Returns (index, delta) of the first delta from `after` on that matches."""
        start = [after]

        def ready():
            for index in range(start[0], len(self.deltas)):
                if predicate(self.deltas[index]):
                    return index, self.deltas[index]
            start[0] = max(start[0], len(self.deltas))  # Resume the scan here next time.
            return None

        return self._wait(ready, timeout, "matching delta")

    def wait_live(self, conversation, predicate, timeout=10, after=0):
        """Waits for a `live` delta of one conversation; returns (index, live)."""
        index, delta = self.wait_delta(
            lambda d: d.get("delta") == "live" and d["id"] == conversation and predicate(d["live"]),
            timeout, after)
        return index, delta["live"]


if __name__ == "__main__":
    # Reference values: FNV-1a 64 test vectors and the Rust path shape.
    assert fnv1a64(b"") == 0xCBF29CE484222325
    assert fnv1a64(b"a") == 0xAF63DC4C8601EC8C
    assert fnv1a64(b"foobar") == 0x85944171F73967E8
    assert config_dir(r"C:\Users\Alice") == r"C:\Users\Alice\.config/adeline"
    assert config_dir("C:\\Users\\Alice\\") == r"C:\Users\Alice\.config/adeline"
    assert config_dir("") == ".config/adeline"
    assert pipe_name(r"C:\Users\Alice") == pipe_name(r"c:\users\alice")
    print(config_dir(r"C:\Users\Alice"), "->", pipe_name(r"C:\Users\Alice"))
    print("This user:", pipe_name(), "exists" if pipe_exists() else "not running")
