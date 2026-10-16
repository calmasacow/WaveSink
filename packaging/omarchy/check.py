#!/usr/bin/env python3
"""Isolated install/reinstall/upgrade/restore checks; no desktop mutations."""
import importlib.util
import json
from pathlib import Path
import tempfile

here = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location("integration", here / "integration.py")
integration = importlib.util.module_from_spec(spec)
spec.loader.exec_module(integration)
payload = {name: (here / name).read_text() for name in integration.ASSETS if name != "launch.json"}
payload["launch.json"] = json.dumps({"command": ["/opt/WaveSink.AppImage"]})

with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    shell = root / "omarchy/shell.json"
    shell.parent.mkdir()
    original = {"version": 1, "idle": {"lock": 123}, "bar": {"layout": {
        "left": [{"id": "user.spaces", "perMonitor": True}],
        "right": [{"id": "user.folder", "widgets": ["omarchy.audio", "user.clock"]}]}}}
    shell.write_text(json.dumps(original))
    assert integration.configure(root, "status")["enabled"] is False
    assert integration.configure(root, "install", payload)["enabled"] is True
    config = json.loads(shell.read_text())
    assert config["bar"]["layout"]["right"][0]["widgets"] == ["wavesink.audio", "user.clock"]
    assert config["idle"] == original["idle"]
    assert config["bar"]["layout"]["left"] == original["bar"]["layout"]["left"]
    stamp = shell.stat().st_mtime_ns
    backup_count = len(list((root / "wavesink/omarchy-panel-backups").iterdir()))
    integration.configure(root, "install", payload)
    assert shell.stat().st_mtime_ns == stamp, "reinstall rewrote shell config"
    assert len(list((root / "wavesink/omarchy-panel-backups").iterdir())) == backup_count
    plugin = root / "omarchy/plugins/wavesink.audio"
    first_entry = json.loads((plugin / "manifest.json").read_text())["entryPoints"]["barWidget"]
    upgraded = payload | {"Model.js": payload["Model.js"] + "\n// upgrade\n"}
    integration.configure(root, "install", upgraded, True)
    next_entry = json.loads((plugin / "manifest.json").read_text())["entryPoints"]["barWidget"]
    assert first_entry != next_entry, "upgrade reused cached QML URL"
    assert (plugin / Path(next_entry).parent / "Model.js").read_text() == upgraded["Model.js"]
    config["idle"]["lock"] = 456
    shell.write_text(json.dumps(config))
    integration.configure(root, "remove")
    restored = json.loads(shell.read_text())
    assert restored["idle"]["lock"] == 456, "restore lost unrelated user changes"
    assert restored["bar"] == original["bar"]
    assert not integration.configure(root, "install", payload, True)["enabled"], "restart undid opt-out"
    assert not (root / "omarchy/plugins/wavesink.audio").exists()

    # Discover any user-owned clone, without personal usernames or device IDs.
    clone = root / "omarchy/plugins/someone.sound/manifest.json"
    clone.parent.mkdir(parents=True)
    clone.write_text(json.dumps({"omarchy": {"clonedFrom": "omarchy.audio"}}))
    original["bar"]["layout"]["right"] = [{"id": "someone.sound", "custom": 7}]
    shell.write_text(json.dumps(original))
    integration.configure(root, "install", payload)
    config = json.loads(shell.read_text())
    assert config["bar"]["layout"]["right"][0] == {"id": "wavesink.audio", "custom": 7}
    # Moving the widget does not lose its previous identity.
    config["bar"]["layout"]["left"].append(config["bar"]["layout"]["right"].pop())
    shell.write_text(json.dumps(config))
    integration.configure(root, "remove")
    assert json.loads(shell.read_text())["bar"]["layout"]["left"][-1]["id"] == "someone.sound"

    shell.write_text("{invalid")
    try:
        integration.configure(root, "install", payload)
        assert False, "malformed config accepted"
    except ValueError:
        assert shell.read_text() == "{invalid"

    # Startup respects a removed widget; explicit Enable repairs it.
    shell.write_text(json.dumps({"bar": {"layout": {"right": [{"id": "user.clock"}]}}}))
    assert not integration.configure(root, "install", payload, True)["enabled"]
    assert integration.configure(root, "install", payload)["enabled"]
    assert json.loads(shell.read_text())["bar"]["layout"]["right"] == [{"id": "user.clock"}, {"id": "wavesink.audio"}]
    integration.configure(root, "remove")
    assert json.loads(shell.read_text())["bar"]["layout"]["right"][-1]["id"] == "omarchy.audio"
    shell.unlink()
    assert integration.configure(root, "install", payload) == {"supported": False, "enabled": False}
print("Panel: install, upgrade, repeated install, nested layouts, moved clones, restore, opt-out, invalid config, other desktops passed")
