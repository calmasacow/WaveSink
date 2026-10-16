#!/usr/bin/env python3
"""Install/restore WaveSink's user-owned Omarchy widget; never touch audio routing."""
import fcntl
import hashlib
import json
import os
import re
from pathlib import Path
import shutil
import sys
import tempfile
import time

PLUGIN = "wavesink.audio"
ASSETS = {"manifest.json", "Panel.qml", "LevelRow.qml", "Model.js", "integration.py", "launch.json"}


def write(path, data):
    path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
    if path.exists() and path.read_bytes() == data:
        return
    fd, temporary = tempfile.mkstemp(dir=path.parent, prefix=".wavesink-")
    try:
        with os.fdopen(fd, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.replace(temporary, path)
    finally:
        if os.path.exists(temporary):
            os.unlink(temporary)


def references(value, path=()):
    if isinstance(value, dict):
        for key, item in value.items():
            yield from references(item, path + (key,))
    elif isinstance(value, list):
        for key, item in enumerate(value):
            yield from references(item, path + (key,))
    elif isinstance(value, str):
        yield path, value


def replace(config, path, value):
    for key in path[:-1]:
        config = config[key]
    config[path[-1]] = value


def configure(root, action, payload=None, automatic=False):
    shell = (root / "omarchy/shell.json").resolve()
    state_dir = root / "wavesink"
    state_dir.mkdir(parents=True, exist_ok=True, mode=0o700)
    with (state_dir / "omarchy-panel.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        state_path = state_dir / "omarchy-panel.json"
        state = json.loads(state_path.read_text()) if state_path.exists() else {}
        if not shell.exists():
            return {"supported": False, "enabled": False}
        original = shell.read_bytes()
        config = json.loads(original)
        layout = config.get("bar", {}).get("layout")
        if not isinstance(layout, dict):
            raise ValueError("Omarchy bar.layout must be an object; configuration left unchanged")
        refs = list(references(layout))
        enabled = any(value == PLUGIN for _, value in refs)
        if action == "status":
            return {"supported": True, "enabled": enabled}
        plugin = root / "omarchy/plugins" / PLUGIN
        if action == "install":
            if automatic and (state.get("disabled") or (state and not enabled)):
                return {"supported": True, "enabled": enabled}
            if not isinstance(payload, dict) or set(payload) != ASSETS or not all(isinstance(v, str) for v in payload.values()):
                raise ValueError("Invalid embedded panel payload")
            # Quickshell caches QML and imported JS. Fresh URLs make upgrades
            # reload every component without restarting the desktop shell.
            generation = "generation-" + hashlib.sha256(json.dumps(payload, sort_keys=True).encode()).hexdigest()[:16]
            manifest = json.loads(payload["manifest.json"])
            manifest["entryPoints"]["barWidget"] = generation + "/Panel.qml"
            payload = {generation + "/" + name: content for name, content in payload.items() if name != "manifest.json"}
            payload["manifest.json"] = json.dumps(manifest, indent=2) + "\n"
            known = {"omarchy.audio"}
            for manifest in (root / "omarchy/plugins").glob("*/manifest.json"):
                try:
                    entry = json.loads(manifest.read_text())
                    if entry.get("omarchy", {}).get("clonedFrom") == "omarchy.audio":
                        known.add(manifest.parent.name)
                except (OSError, ValueError):
                    continue
            changes = [(path, value) for path, value in refs if value in known]
            if not enabled and not changes:
                # Automatic startup respects removed widgets. Explicit Enable
                # can put the panel back even when no audio widget remains.
                if automatic:
                    return {"supported": True, "enabled": False}
                section = layout.setdefault("right", [])
                if not isinstance(section, list):
                    raise ValueError("Omarchy bar.layout.right must be a list")
                section.append({"id": "omarchy.audio"})
                changes = [(("right", len(section) - 1, "id"), "omarchy.audio")]
            if enabled and not changes and all((plugin / name).is_file() and (plugin / name).read_text() == content for name, content in payload.items()):
                return {"supported": True, "enabled": True}
            backup = state_dir / "omarchy-panel-backups" / str(time.time_ns())
            backup.mkdir(parents=True, mode=0o700)
            (backup / "shell.json").write_bytes(original)
            if plugin.exists():
                shutil.copytree(plugin, backup / PLUGIN)
            for filename, content in payload.items():
                write(plugin / filename, content.encode())
            old = state.get("replaced", []) if enabled else []
            for path, value in changes:
                old.append({"path": list(path), "id": value})
                replace(layout, path, PLUGIN)
            hashes = state.get("hashes", {})
            hashes.update({k: hashlib.sha256(v.encode()).hexdigest() for k, v in payload.items()})
            state.update(disabled=False, replaced=old, hashes=hashes)
        elif action == "remove":
            old = state.get("replaced", [])
            unique = {item["id"] for item in old}
            for path, value in refs:
                if value == PLUGIN:
                    prior = next((x["id"] for x in old if x["path"] == list(path)), None)
                    replace(layout, path, prior or (next(iter(unique)) if len(unique) == 1 else "omarchy.audio"))
            state["disabled"] = True
        else:
            raise ValueError("Expected install, remove, or status")
        if shell.read_bytes() != original:
            raise RuntimeError("Omarchy configuration changed during setup; retry")
        # Save rollback information before activating the new widget.
        write(state_path, (json.dumps(state, indent=2) + "\n").encode())
        if config != json.loads(original):
            write(shell, (json.dumps(config, indent=2) + "\n").encode())
        if action == "remove":
            for filename, digest in state.get("hashes", {}).items():
                parts = Path(filename).parts
                safe = filename in ASSETS or (len(parts) == 2 and re.fullmatch(r"generation-[0-9a-f]{16}", parts[0]) and parts[1] in ASSETS)
                if safe:
                    path = plugin / filename
                    if path.is_file() and hashlib.sha256(path.read_bytes()).hexdigest() == digest:
                        path.unlink()
                        if path.parent != plugin and not any(path.parent.iterdir()):
                            path.parent.rmdir()
            if plugin.exists() and not any(plugin.iterdir()):
                plugin.rmdir()
        return {"supported": True, "enabled": action == "install"}


if __name__ == "__main__":
    try:
        if os.getuid() == 0:
            raise ValueError("Run panel setup as your desktop user, never root")
        action = sys.argv[1]
        root = Path(os.environ.get("XDG_CONFIG_HOME", str(Path.home() / ".config")))
        payload = json.load(sys.stdin) if action == "install" else None
        result = configure(root, action, payload, "--automatic" in sys.argv)
        print(json.dumps(result))
    except (OSError, ValueError, KeyError, RuntimeError, IndexError) as error:
        print(f"WaveSink panel: {error}", file=sys.stderr)
        sys.exit(1)
