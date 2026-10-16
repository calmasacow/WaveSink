#!/usr/bin/env python3
"""Test capture pins on virtual devices in a private server, with no hardware monitors."""
import json
import os
from pathlib import Path
import subprocess
import shutil
import sys
import tempfile
import time

repo = Path(__file__).resolve().parents[1]
# Isolate D-Bus as well as PipeWire and WirePlumber state.
if "--private-bus" not in sys.argv:
    raise SystemExit(subprocess.call(["dbus-run-session", "--", sys.executable, __file__, "--private-bus"]))

with tempfile.TemporaryDirectory(prefix="wavesink-elgato-", dir="/tmp") as directory:
    for name in ["pipewire.conf", "client.conf"]:
        shutil.copyfile(Path("/usr/share/pipewire") / name, Path(directory) / name)
    config = Path(directory) / "pipewire.conf"
    config.write_text(config.read_text().replace("#audiotestsrc   =", "audiotestsrc   ="))
    env = dict(os.environ, XDG_RUNTIME_DIR=directory, PIPEWIRE_RUNTIME_DIR=directory,
               PIPEWIRE_REMOTE="wavesink-elgato-test", PIPEWIRE_CORE="wavesink-elgato-test", PIPEWIRE_CONFIG_DIR=directory, XDG_CONFIG_HOME=directory,
               XDG_STATE_HOME=directory, XDG_CACHE_HOME=directory, GIO_USE_VFS="local", GSETTINGS_BACKEND="memory")
    processes = []
    state_dir = Path(directory) / "wireplumber"
    state_dir.mkdir()
    # Reproduce stale internal FX volume/mute restored when the gate creates streams.
    (state_dir / "stream-properties").write_text(
        "[stream-properties]\n" + "".join(
            f'{direction}/Audio:media.name:sink-internal-fx-{stage}-test-mic='
            '{"mute":true,"volume":1.0,"channelVolumes":[0.000083,0.000083],"channelMap":["FL","FR"]}\n'
            for direction, stage in [("Input", "capture"), ("Output", "playback")]
        )
    )
    with open(Path(directory) / "daemon.log", "w+") as log:
        try:
            processes.append(subprocess.Popen(["pipewire", "-P", "{ core.name = wavesink-elgato-test }"], env=env, stdout=log, stderr=log))
            for _ in range(100):
                if (Path(directory) / "wavesink-elgato-test").exists():
                    break
                time.sleep(0.03)
            else:
                raise RuntimeError(f"Private PipeWire socket did not appear: {os.listdir(directory)}")
            # Built-in policy profile has no ALSA/Bluetooth/video device monitors.
            processes.append(subprocess.Popen(["wireplumber", "--profile", "policy"], env=env, stdout=log, stderr=log))
            for kind, channels, position in [("source", 1, "MONO"), ("output", 2, "FL FR")]:
                factory = "audiotestsrc" if kind == "source" else "support.null-audio-sink"
                props = (f"{{ factory.name = {factory} object.linger = true "
                         f"node.name = wavesink-test-dock-{kind} "
                         f"media.class = Audio/{'Source' if kind == 'source' else 'Sink'} "
                         f"audio.channels = {channels} audio.position = [ {position} ] "
                         'device.api = alsa api.alsa.pcm.card = 99999 alsa.components = "USB0fd9:00a6" }')
                subprocess.run(["pw-cli", "create-node", "adapter", props], env=env, check=True, stdout=subprocess.DEVNULL)
            dump = json.loads(subprocess.check_output(["pw-dump"], env=env))
            assert sum(o.get("info", {}).get("props", {}).get("node.name", "").startswith("wavesink-test-dock-") for o in dump) == 2, "Virtual nodes failed to appear"
            assert not any(o.get("info", {}).get("props", {}).get("api.alsa.path") for o in dump), "Real ALSA node in private server"
            subprocess.run(["cargo", "test", "--lib", "--no-default-features", "isolated_capture_first", "--", "--ignored", "--nocapture"], cwd=repo, env=env, check=True)
        except BaseException:
            log.seek(0)
            print(log.read()[-6000:])
            raise
        finally:
            for process in reversed(processes):
                process.terminate()
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
