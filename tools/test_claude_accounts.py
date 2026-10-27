#!/usr/bin/env python3
"""Read-only live discovery check plus a temporary, credential-free Claude profile.

Uses a tiny sleep executable as a synthetic live process. Does not launch
Claude, change its login/settings, or send requests to any provider.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time
import uuid


def snapshot(binary, env):
    result = subprocess.run(
        [str(binary), "--session-info"], env=env,
        capture_output=True, text=True, check=True,
    )
    return [s for s in json.loads(result.stdout)["sessions"] if s["kind"] == "claude"]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path(__file__).resolve().parents[1] / "target/release/token-cafe")
    args = parser.parse_args()
    binary = args.binary.resolve()
    clean_env = os.environ.copy()
    clean_env.pop("CLAUDE_CONFIG_DIR", None)
    clean_env.pop("TOKEN_CAFE_CLAUDE_CONFIG_DIRS", None)
    baseline = snapshot(binary, clean_env)
    print("Live Claude sessions:", len(baseline))
    for session in baseline:
        # Never print transcript excerpts, credentials, commands or email addresses.
        identity = session.get("claude") or {}
        print(json.dumps({"id": session["id"], "surface": identity.get("surface"),
                          "account": identity.get("account"),
                          "transcript_available": session["transcript_available"]}))

    with tempfile.TemporaryDirectory(prefix="token-cafe-accounts-") as temp:
        root = Path(temp).resolve()
        executable = root / "claude"
        subprocess.run(["cc", "-x", "c", "-", "-o", str(executable)],
            input="#include <unistd.h>\nint main(void) { sleep(120); return 0; }\n",
            text=True, capture_output=True, check=True)
        profile = root / "second profile"
        sessions = profile / "sessions"
        project = profile / "projects/moon"
        sessions.mkdir(parents=True)
        project.mkdir(parents=True)
        sid = str(uuid.uuid4())
        (profile / ".claude.json").write_text(json.dumps({"oauthAccount": {
            "accountUuid": "fixture-user", "organizationUuid": "fixture-org"}}))
        (project / f"{sid}.jsonl").write_text(json.dumps({"type": "user", "sessionId": sid,
            "message": {"content": "Synthetic second-account task"}}) + "\n")
        child = subprocess.Popen([str(executable), "120"], env={**clean_env, "CLAUDE_CONFIG_DIR": str(profile)})
        try:
            (sessions / f"{child.pid}.json").write_text(json.dumps({"pid": child.pid,
                "sessionId": sid, "startedAt": time.time() * 1000, "cwd": str(root),
                "entrypoint": "cli", "kind": "interactive", "status": "waiting"}))
            for override in (False, True):
                env = clean_env.copy()
                if override:
                    alias = root / "alias"
                    alias.symlink_to(profile, target_is_directory=True)
                    env.update(CLAUDE_CONFIG_DIR=str(alias), TOKEN_CAFE_CLAUDE_CONFIG_DIRS=f"{profile}:{alias}")
                rows = snapshot(binary, env)
                matches = [s for s in rows if s["id"] == sid]
                if len(matches) != 1:
                    explicit = snapshot(binary, {**env, "CLAUDE_CONFIG_DIR": str(profile)})
                    raise AssertionError(f"Second profile missing or counted twice; auto={len(matches)}, explicit={sum(s['id'] == sid for s in explicit)}, process_exit={child.poll()}")
                row = matches[0]
                assert row["claude"]["config_dir"] == str(profile)
                assert row["claude"]["surface"] == "cli"
                assert row["claude"]["account"] == {"account_id": "fixture-user",
                    "organization_id": "fixture-org", "source": "current_config"}
                assert row["state"] == "wait"
                assert row["task"]["text"] == "Synthetic second-account task"
                assert row["transcript_available"]
                # A per-app override must not hide default-profile sessions.
                assert any(s["id"] != sid for s in rows) or not baseline
                print("PASS:", "override + alias deduplication" if override else "automatic process config discovery")
        finally:
            if child.poll() is None:
                child.terminate()
            child.wait(timeout=5)
        assert all(s["id"] != sid for s in snapshot(binary, clean_env)), "Closed fixture still appears live"
        print("PASS: closed profile process excluded")


if __name__ == "__main__":
    main()
