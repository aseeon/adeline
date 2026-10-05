"""A stand-in for `ssh` in UI checks: every destination is another Windows
machine with OpenSSH Server, simulated on this computer.

Adeline runs it when ADELINE_SSH is "<python> <this file>". A destination's
home is ADELINE_FAKE_SSH_ROOT/<host>. The remote command runs there through
cmd.exe, as Windows OpenSSH runs it, with USERPROFILE and HOME pointing at that
home and Git's tools off PATH, like a plain Windows host. Its engine is
therefore a separate engine with its own pipe.

Hosts (the part after any `user@`):
  down...     the connection times out
  changed...  the host key changed
  pw-...      asks for a password through SSH_ASKPASS; "secret" passes
  newkey-...  asks once to trust its host key (remembered in ROOT/known_hosts)
  arm...      reports an ARM64 processor
  other       connects
A file ROOT/<host>.alias holding another host's name makes both reach one home.
"""
import os
import subprocess
import sys
from pathlib import Path


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
    env = {k: v for k, v in os.environ.items() if not k.startswith(("ADELINE_ASKPASS", "SSH_ASKPASS"))}
    env.update(USERPROFILE=str(home), HOME=str(home),
               PATH=os.pathsep.join(p for p in env["PATH"].split(os.pathsep) if "\\git\\" not in p.lower()))
    if host.startswith("arm"):
        env["PROCESSOR_ARCHITECTURE"] = "ARM64"
    result = subprocess.run(f"cmd.exe /c {command}", cwd=home, env=env,
                            stdin=sys.stdin, stdout=sys.stdout, stderr=sys.stderr)
    sys.exit(result.returncode)


if __name__ == "__main__":
    main()
