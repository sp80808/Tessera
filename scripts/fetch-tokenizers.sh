#!/usr/bin/env bash
# Fetch HF tokenizer.json artifacts listed in bench/tokenizers.json that have
# sha256 + revision pins. Verifies the hash. Does not commit files (gitignored).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
MANIFEST="${1:-$ROOT/bench/tokenizers.json}"
DEST="${2:-$ROOT/bench/tokenizers}"
python3 - "$MANIFEST" "$DEST" <<'PY'
import hashlib, json, sys, urllib.request
from pathlib import Path

manifest_path, dest = Path(sys.argv[1]), Path(sys.argv[2])
doc = json.loads(manifest_path.read_text())
fetched = 0
for entry in doc["tokenizers"]:
    file = entry.get("file") or {}
    sha, rev, rel = file.get("sha256"), file.get("revision"), file.get("local_path")
    repo, name = file.get("repo_id"), file.get("hub_filename")
    if not (sha and rev and rel and repo and name):
        continue
    out = dest / rel
    out.parent.mkdir(parents=True, exist_ok=True)
    url = f"https://huggingface.co/{repo}/resolve/{rev}/{name}"
    print(f"fetch {entry['id']} <- {url}", flush=True)
    with urllib.request.urlopen(url, timeout=120) as resp:
        data = resp.read()
    digest = hashlib.sha256(data).hexdigest()
    if digest != sha:
        raise SystemExit(f"sha256 mismatch for {entry['id']}: expected {sha}, got {digest}")
    out.write_bytes(data)
    fetched += 1
    print(f"  ok {out} ({len(data)} bytes)", flush=True)
print(f"fetched {fetched} pinned tokenizer artifact(s)")
PY
