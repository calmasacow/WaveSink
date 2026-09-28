# Fedora packaging

`wavesink.spec` builds the Fedora package for WaveSink.
It repackages the release `.deb` (no source build), so it installs with system PipeWire and webkit2gtk and no toolchain.

Download the `.rpm` from a [WaveSink release](https://github.com/calmasacow/WaveSink/releases), then install it:

```bash
sudo dnf install ./wavesink-*.rpm
```

The linked-library `Requires` come from the binary's sonames automatically; only the runtime services and the dlopen'd tray lib are declared by hand.
