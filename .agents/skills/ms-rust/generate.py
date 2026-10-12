"""Download and split Microsoft Rust Guidelines into Agent Skill files."""

import hashlib
import re
import sys
from datetime import date
from pathlib import Path

import httpx
from jinja2 import Environment, FileSystemLoader

GUIDELINES_URL = "https://microsoft.github.io/rust-guidelines/agents/all.txt"
REPO_ROOT = Path(__file__).resolve().parent
TEMPLATE_DIR = REPO_ROOT / "templates"
STALE_RE = re.compile(r"^\d{2}_.*\.md$")
HASH_FILE = REPO_ROOT / "all.txt.sha256"
COMPLIANCE_DATE_RE = re.compile(r"\*\*Current compliance date: (\d{4}-\d{2}-\d{2})\*\*")


def content_hash(content: str) -> str:
    """Return the SHA-256 hex digest of the given content."""
    return hashlib.sha256(content.encode()).hexdigest()


def read_stored_hash() -> str | None:
    """Read the previously stored guidelines hash, if any."""
    if HASH_FILE.is_file():
        return HASH_FILE.read_text(encoding="utf-8").strip()
    return None


def read_existing_compliance_date() -> str | None:
    """Extract the compliance date from the current SKILL.md, if it exists."""
    skill_path = REPO_ROOT / "SKILL.md"
    if skill_path.is_file():
        match = COMPLIANCE_DATE_RE.search(skill_path.read_text(encoding="utf-8"))
        if match:
            return match.group(1)
    return None


def download_guidelines() -> str:
    """Fetch the guidelines text from Microsoft's URL."""
    response = httpx.get(GUIDELINES_URL, timeout=30, follow_redirects=True)
    response.raise_for_status()
    return response.text


def clean_stale_files() -> None:
    """Remove previously generated NN_*.md files from the repo root."""
    for path in REPO_ROOT.iterdir():
        if path.is_file() and STALE_RE.match(path.name):
            path.unlink()
            print(f"Removed: {path.name}")


def split_guidelines(content: str) -> list[dict]:
    """Split the all.txt content into per-section markdown files.

    Returns a list of dicts with 'name' (filename) and 'title' keys.
    """
    lines = content.splitlines()

    # Find all separator line indices
    sep_indices = [i for i, line in enumerate(lines) if line.startswith("---")]

    if len(sep_indices) < 2:
        print("No section separators found; nothing to extract.")
        return []

    files = []
    counter = 1

    for k in range(len(sep_indices) - 1):
        start = sep_indices[k] + 1
        end = sep_indices[k + 1]
        section = lines[start:end]

        # Find first H1 heading in the section
        h1_idx = None
        for j, line in enumerate(section):
            if re.match(r"^[ \t]*#\s+", line):
                h1_idx = j
                break

        if h1_idx is None:
            continue

        extract = [l for l in section[h1_idx:] if not l.startswith("---")]

        # Derive filename from heading
        title = re.sub(r"^[ \t]*#\s+", "", extract[0]).strip()
        slug = re.sub(r"[\s/]+", "_", title.lower())
        slug = re.sub(r"[^a-z0-9_]+", "_", slug).strip("_") or "section"

        filename = f"{counter:02d}_{slug}.md"
        out_path = REPO_ROOT / filename
        out_path.write_text("\n".join(extract) + "\n", encoding="utf-8")
        print(f"Wrote: {filename}")

        files.append({"name": filename, "title": title})
        counter += 1

    return files


GUIDELINE_DESCRIPTIONS = {
    "ai": "Use when the Rust code involves AI agents or LLM-driven code generation, making APIs easier for AI systems, avoiding meta design documentation, writing idiomatic Rust-shaped code, keeping a single public path per item, or writing tests that do not merely assert ground truth.",
    "application": "Use when working on application-level error handling with anyhow or eyre, CLI tools, desktop applications, performance optimization using the mimalloc allocator or target-cpu settings, or user-facing features.",
    "correctness": "Use when deciding between panics and errors, writing panic messages, handling panic continuation, writing or reviewing unsafe code, ensuring soundness, or preventing undefined behavior.",
    "documentation": "Use when writing public API documentation and doc comments, creating canonical documentation sections (Examples, Errors, Panics, Safety), structuring module-level documentation, or using #[doc(inline)] annotations.",
    "ffi": "Use when loading multiple Rust-based dynamic libraries, creating FFI boundaries, sharing data between different Rust compilation artifacts, or dealing with portable vs non-portable data types across DLL boundaries.",
    "library_guidelines": "Use when creating or modifying Rust libraries, structuring a crate, designing public APIs, or making dependency decisions.",
    "macros": "Use when writing declarative or procedural macros, deciding whether a macro is warranted, structuring proc macro crates, or exposing macro helper items.",
    "performance": "Use when profiling hot paths, optimizing for throughput and CPU efficiency, managing allocation patterns and memory usage, or implementing yield points in long-running async tasks.",
    "project": "Use when setting up or modifying a Cargo workspace, laying out crate folders, declaring workspace dependencies, choosing the Rust edition, or updating the MSRV.",
    "safety": "Section heading only; the unsafe code and soundness rules (M-UNSAFE, M-UNSAFE-IMPLIES-UB, M-UNSOUND) now live in the correctness guidelines.",
    "universal": "Use in ALL Rust tasks. Defines general best practices, style, naming, organizational conventions, and foundational principles.",
    "building": "Use when creating reusable library crates, managing Cargo features, building native -sys crates for C interop, or ensuring libraries work out-of-the-box on all platforms.",
    "interoperability": "Use when exposing public APIs, managing external dependencies, designing types for Send/Sync compatibility, avoiding leaking third-party types, or creating escape hatches for native handle interop.",
    "resilience": "Use when writing unit or integration tests (integration tests go under tests/), feature-gating test utilities, making I/O mockable, avoiding statics and thread-local state, preventing glob re-exports, validating builders in .build(), making newtypes enforce their invariants, or emitting diagnostics via telemetry instead of println.",
    "ux": "Use when designing user-friendly library APIs, managing error types, creating runtime abstractions and trait-based designs, or structuring crate organization.",
}


def match_description(slug: str) -> str:
    """Match a generated filename slug to its guideline description."""
    for key, desc in GUIDELINE_DESCRIPTIONS.items():
        if key in slug:
            return desc
    return "Consult this guideline when relevant to your task."


def render_skill_md(files: list[dict], compliance_date: str) -> None:
    """Render SKILL.md from the Jinja2 template."""
    env = Environment(
        loader=FileSystemLoader(TEMPLATE_DIR),
        keep_trailing_newline=True,
    )
    template = env.get_template("SKILL.md.j2")

    guideline_files = []
    for f in files:
        guideline_files.append({
            "name": f["name"],
            "description": match_description(f["name"]),
        })

    output = template.render(
        compliance_date=compliance_date,
        guideline_files=guideline_files,
    )

    out_path = REPO_ROOT / "SKILL.md"
    out_path.write_text(output, encoding="utf-8")
    print("Wrote: SKILL.md")


def main() -> None:
    """Download, split, and render the skill files."""
    print(f"Downloading guidelines from {GUIDELINES_URL}...")
    content = download_guidelines()
    print(f"Downloaded {len(content)} characters.")

    new_hash = content_hash(content)
    stored_hash = read_stored_hash()
    guidelines_changed = new_hash != stored_hash

    if guidelines_changed:
        compliance_date = date.today().isoformat()
        print("Guidelines changed — updating compliance date.")
    else:
        compliance_date = read_existing_compliance_date() or date.today().isoformat()
        print("Guidelines unchanged — keeping existing compliance date.")

    clean_stale_files()

    files = split_guidelines(content)
    if not files:
        print("No files generated.", file=sys.stderr)
        sys.exit(1)

    render_skill_md(files, compliance_date)

    HASH_FILE.write_text(new_hash + "\n", encoding="utf-8")
    print(f"\nDone. Generated {len(files)} guideline file(s) + SKILL.md."
          f" Compliance date: {compliance_date}")


if __name__ == "__main__":
    main()
