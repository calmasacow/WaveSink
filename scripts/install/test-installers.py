#!/usr/bin/env python3
"""Run installer flows against a fake release and fake package managers."""

import hashlib
import os
import pathlib
import subprocess
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent
TAG = "v9.8.7"
ARTIFACTS = {
    "arch": "wavesink-bin-9.8.7-1-x86_64.pkg.tar.zst",
    "ubuntu": "wavesink_9.8.7_amd64.deb",
    "fedora": "wavesink-9.8.7-1.x86_64.rpm",
    "nixos": "flake.nix",
    "appimage": "wavesink_9.8.7_amd64.AppImage",
}

with tempfile.TemporaryDirectory() as tmp:
    root = pathlib.Path(tmp)
    bin_dir = root / "bin"
    bin_dir.mkdir()
    (root / "os-release").write_text('ID="fedora"\nVERSION_ID="43"\n')
    payloads = {name: f"fixture:{name}\n".encode() for name in ARTIFACTS.values()}
    manifest = "".join(
        f"{hashlib.sha256(payload).hexdigest()}  {name}\n"
        for name, payload in payloads.items()
    ).encode()
    (root / "SHA256SUMS").write_bytes(manifest)
    for name, payload in payloads.items():
        (root / name).write_bytes(payload)

    (bin_dir / "curl").write_text(
        '#!/bin/bash\n'
        'if [[ "$*" == *"%{url_effective}"* ]]; then '
        f'printf "https://github.com/calmasacow/WaveSink/releases/tag/{TAG}"; exit; fi\n'
        'while (($#)); do if [[ "$1" == -o ]]; then out="$2"; shift 2; '
        'else url="$1"; shift; fi; done\n'
        'cp "$FIXTURES/${url##*/}" "$out"\n'
    )
    for name in ("sudo", "nix", "pacman", "apt", "dnf"):
        (bin_dir / name).write_text(
            '#!/bin/bash\nprintf "%s %s\\n" "${0##*/}" "$*" >> "$CALLS"\n'
        )
    for file in bin_dir.iterdir():
        file.chmod(0o755)

    for platform, asset in ARTIFACTS.items():
        script = (ROOT / f"{platform}.sh").read_text()
        script = script.replace(". /etc/os-release", f'. "{root / "os-release"}"')
        if platform == "ubuntu":
            (root / "os-release").write_text('ID="ubuntu"\nVERSION_ID="24.04"\n')
        elif platform == "nixos":
            (root / "os-release").write_text('ID="nixos"\nVERSION_ID="26.05"\n')
        else:
            (root / "os-release").write_text('ID="fedora"\nVERSION_ID="43"\n')
        calls = root / "calls"
        calls.write_text("")
        env = os.environ | {
            "PATH": f"{bin_dir}:{os.environ['PATH']}",
            "FIXTURES": str(root),
            "CALLS": str(calls),
            "XDG_DATA_HOME": str(root / "data"),
        }
        result = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True)
        assert result.returncode == 0, (platform, result.stderr)
        if platform == "appimage":
            assert (root / "data/AppImages/WaveSink.AppImage").read_bytes() == payloads[asset]
        else:
            assert asset in calls.read_text() or platform == "nixos", (platform, calls.read_text())
        payloads[asset] = b"corrupt"
        (root / asset).write_bytes(b"corrupt")
        calls.write_text("")
        result = subprocess.run(["bash", "-c", script], env=env, capture_output=True, text=True)
        assert result.returncode != 0 and "checksum" in result.stderr.lower(), platform
        assert not calls.read_text(), platform
        (root / asset).write_bytes(f"fixture:{asset}\n".encode())
        print(f"{platform}: verified install and checksum rejection")
