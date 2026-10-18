"""A stand-in for `ssh` in the remote tests: every destination is another machine
with OpenSSH Server and this computer's OS, simulated on this computer.

Adeline runs it when ADELINE_SSH is "<python> <this file>". A destination's
home is ADELINE_FAKE_SSH_ROOT/<host>. The remote command runs there the way
OpenSSH runs it on this OS: through cmd.exe on Windows, with Git's tools off
PATH like a plain Windows host, and through `sh -c` elsewhere. USERPROFILE and
HOME point at that home, so its engine is a separate engine with its own pipe
or socket.

Hosts (the part after any `user@`):
  down...     the connection times out
  changed...  the host key changed
  pw-...      asks for a password through SSH_ASKPASS; "secret" passes
  newkey-...  asks once to trust its host key (remembered in ROOT/known_hosts)
  arm...      reports an ARM processor Adeline has no build for
  other       connects
A file ROOT/<host>.alias holding another host's name makes both reach one home.
While a bridge runs, ROOT/bridges/<this process id> holds the process id of the
command's shell (a process group on Unix), so a test can drop the connection.
"""
import os
import subprocess
import sys
from pathlib import Path

WINDOWS = os.name == "nt"


def fail(message):
    print(message, file=sys.stderr, flush=True)
    sys.exit(255)


def ask(prompt):
    """Runs Adeline's askpass the way ssh does; returns the answer or None."""
    result = subprocess.run([os.environ["SSH_ASKPASS"], prompt], capture_output=True, text=True)
    return result.stdout.rstrip("\r\n") if result.returncode == 0 else None


def main():
    args = sys.argv[1:]
    while args and args[0].startswith("-") and args[0] != "--":
        args = args[2:] if args[0] in ("-p", "-o") else args[1:]
    if args and args[0] == "--":
        args = args[1:]
    destination, command = args[0], args[1]
    host = destination.rsplit("@", 1)[-1]
    root = Path(os.environ["ADELINE_FAKE_SSH_ROOT"])
    if host.startswith("down"):
        fail(f"ssh: connect to host {host} port 22: Connection timed out")
    if host.startswith("changed"):
        fail("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n"
             "@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @\n"
             "@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@\n"
             f"Host key for {host} has changed and you have requested strict checking.\n"
             "Host key verification failed.")
    if host.startswith("newkey"):
        known = root / "known_hosts"
        hosts = known.read_text().split() if known.exists() else []
        if host not in hosts:
            answer = ask(f"The authenticity of host '{host} (127.0.0.1)' can't be established.\n"
                         "ED25519 key fingerprint is SHA256:Q2xhdWRlIGZha2Ugc3NoIGhvc3Qga2V5.\n"
                         "This key is not known by any other names.\n"
                         "Are you sure you want to continue connecting (yes/no/[fingerprint])? ")
            if answer != "yes":
                fail("Host key verification failed.")
            known.write_text("\n".join(hosts + [host]))
    if host.startswith("pw-"):
        for _ in range(3):
            if ask(f"{destination}'s password: ") == "secret":
                break
            print("Permission denied, please try again.", file=sys.stderr, flush=True)
        else:
            fail(f"{destination}: Permission denied (publickey,password).")
    alias = root / f"{host}.alias"
    if alias.exists():
        host = alias.read_text().strip()
    home = root / host
    home.mkdir(parents=True, exist_ok=True)
    # A remote machine has its own environment, not the client's.
    env = {k: v for k, v in os.environ.items() if not k.startswith(("ADELINE_ASKPASS", "ADELINE_EXE", "SSH_ASKPASS"))}
    env.update(USERPROFILE=str(home), HOME=str(home))
    if WINDOWS:
        env["PATH"] = os.pathsep.join(p for p in env["PATH"].split(os.pathsep) if "\\git\\" not in p.lower())
        if host.startswith("arm"):
            env["PROCESSOR_ARCHITECTURE"] = "ARM64"
        argv = f"cmd.exe /c {command}"
    else:
        if host.startswith("arm"):
            tools = root / "arm-tools"
            tools.mkdir(exist_ok=True)
            uname = tools / "uname"
            uname.write_text("#!/bin/sh\necho Linux armv7l\n")
            uname.chmod(0o755)
            env["PATH"] = f"{tools}{os.pathsep}{env['PATH']}"
        argv = ["/bin/sh", "-c", command]
    # Its own process group on Unix, so a test can drop the whole session.
    child = subprocess.Popen(argv, cwd=home, env=env, stdin=sys.stdin, stdout=sys.stdout, stderr=sys.stderr,
                             start_new_session=not WINDOWS)
    record = root / "bridges" / str(os.getpid())
    if "bridge" in command:
        record.parent.mkdir(exist_ok=True)
        record.write_text(str(child.pid))
    try:
        sys.exit(child.wait())
    finally:
        record.unlink(missing_ok=True)


if __name__ == "__main__":
    main()
