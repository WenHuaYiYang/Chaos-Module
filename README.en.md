<div align="center">

# Chaos

**A native application platform for the Xiaomi Smart Band 10 Pro**

A kernel module plus a Lua installer that registers native pages through the firmware's own registration chain.

[简体中文](README.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Firmware](https://img.shields.io/badge/firmware-3.101.043-green.svg)](#compatibility)
[![Device](https://img.shields.io/badge/device-p67-orange.svg)](#compatibility)
[![Release](https://img.shields.io/badge/release-v1.0.3-blue.svg)](docs/CHANGELOG.md)

</div>

---

## Overview

Chaos registers a set of Rust-written native pages with the Xiaomi Smart Band 10 Pro firmware so they appear
on the home screen as ordinary apps. Icons, taps and transitions are handled by the stock launcher.
The module is offline, needs no phone, and does not touch firmware partitions.

It started as a research harness for the 10 Pro and grew into a full device-management UI:
apps, watchfaces, fonts, icons, brightness, cache, blur and vibration.

> **Scale**: ~14k lines of Rust, 19 page ids, a `chaos_sup.ko` of roughly 190 KB.

## Features

- **App manager**: hide/show, batch actions, exclusions, and normal / deep hiding modes ([details](docs/APP-MANAGER.md)).
- **Customization**: fonts, home-screen icon packs, system blur, translucent watchface background.
- **Watchfaces**: switch built-in/market faces, shake-to-rotate, face management.
- **Device tools**: brightness, cache cleanup, time/date, vibration test.
- **Runtime log**: key actions go to `/data/chaos/chaos.log`; Settings → Runtime log shows and clears it.
- **No external runtime**: talks only to verified firmware APIs; no JS injection, no network.

## Compatibility

| Item | Requirement |
|---|---|
| Device | Xiaomi Smart Band 10 Pro (`p67` / `M2551B1`) |
| Firmware | **3.101.043** |
| Module format | ARM ELF32 `ET_REL`, `thumbv8m.main-none-eabi`, soft-float |
| Installer | Lua container (magic `0x1234A55A`) |

> Firmware addresses are hard-coded. **Do not use this module on 3.101.036 or any other firmware.**
> Upgrading the firmware requires re-adapting the address tables.

## Quick start

1. Download `chaos-installer-p67.bin` from [Releases](https://github.com/WenHuaYiYang/Chaos-Module/releases).
2. Install it through AstroBox's custom-watchface (or third-party app) entry.
3. **Reboot the band** so the module loads at boot.
4. Open Chaos and enable auto-injection / background restore as needed.

Full steps and troubleshooting: **[docs/INSTALL.md](docs/INSTALL.md)**.

## Build

```bash
python3 tools/build.py            # ko + icons + p67 container
python3 tools/build.py --skip-ko  # re-pack the container only
```

Dependencies, pipeline, versioning and release checklist: **[docs/BUILD.md](docs/BUILD.md)**.

## Documentation

| Document | Contents |
|---|---|
| [Install & upgrade](docs/INSTALL.md) | Install, rollback, common failures |
| [Usage](docs/USAGE.md) | Page map and every feature |
| [App manager](docs/APP-MANAGER.md) | Firmware evidence for both hiding modes |
| [Build & release](docs/BUILD.md) | Toolchain, pipeline, versioning, CI |
| [Architecture](docs/ARCHITECTURE.md) | Boot chain, page model, IPC, firmware coupling |
| [LVGL map](docs/LVGL-MAP.md) | LVGL classes and entry points on p67 |
| [Changelog](docs/CHANGELOG.md) | Version history |

Documentation index: [docs/README.md](docs/README.md).

## Contributing

1. Fork the repository and branch from `master`.
2. Run `sh tools/build_ko.sh` (or wait for CI) after module changes.
3. For changes touching firmware addresses, page counts or registration chains, include evidence in the PR
   (device logs or disassembly).
4. Make sure no `.ko`, font, commercial asset or secret is committed.

## License

[AGPL-3.0](LICENSE). Redistributed modifications must keep the same license and source offer.

Fonts and third-party assets follow their own licenses; this repository ships no commercial fonts,
system icons or firmware images.

## Acknowledgements

- The Xiaomi Vela / NuttX community and the openvela documentation
- Everyone who validated changes on real hardware and contributed logs or disassembly evidence

<div align="center">

**If this project helps you, consider giving it a Star ⭐**

</div>
