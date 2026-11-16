<div align="center">

# Chaos

**A native application platform for the Xiaomi Smart Band 10 Pro**

A kernel module plus a Lua installer that registers native pages through the firmware's own registration chain.

[简体中文](README.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Firmware](https://img.shields.io/badge/firmware-3.101.043-green.svg)](#compatibility)
[![Device](https://img.shields.io/badge/device-p67-orange.svg)](#compatibility)
[![Release](https://img.shields.io/badge/version-v1.2.4-blue.svg)](docs/CHANGELOG.md)

</div>

---

## Overview

Chaos registers a set of Rust-written native pages with the Xiaomi Smart Band 10 Pro firmware so they appear
on the home screen as ordinary apps. Icons, taps and transitions are handled by the stock launcher.
Core management works without a phone. Networking and ChaosConnect are optional; the module does not touch firmware partitions.

It started as a research harness for the 10 Pro and grew into a full device-management UI:
apps, watchfaces, fonts, icons, brightness, cache, blur and vibration.

> **Implementation**: native Rust (`no_std`) and C modules, and a Lua installer.

## Features

- **App manager**: hide/show, batch actions, exclusions, and normal / deep hiding modes ([details](docs/APP-MANAGER.md)).
- **Customization**: fonts, static/animated home-screen icon packs, system blur, translucent watchface background. Animated packs keep legacy CIPK compatibility and play all visible animated icons ([delivery guide and format](docs/ICON-PACKS.md)).
- **Honeycomb app list (experimental)**: two-axis dragging, inertia, independent left-edge spacing, right-edge zoom, boundary navigation and native editing/deletion. The switch persists across restarts; switching between honeycomb and RW6 rebinds the selected layout immediately and keeps them mutually exclusive. Full simulator rendering and frame-time validation remain pending ([implementation and tests](docs/HONEYCOMB.md)).
- **Watchfaces**: switch built-in/market faces, shake-to-rotate, face management.
- **Device tools**: brightness, cache cleanup, time/date, vibration test.
- **Application icon**: Settings → Application icon offers Original, Dynamic, Dark potato and Dark icon at 96×96. Dynamic is the default and retains 193 frames with a 5.2-second cycle; manual choices persist ([details](docs/DYNAMIC-ICON.md)).
- **ChaosConnect operation log**: a separate 32KiB RAM log records connection and file operations. The generic flash runtime log has been removed.
- **Screenshots and recordings**: background GIF 15 fps / APNG target 60 fps recording, BMP capture after screen-off, independent storage choices (`/data/chaos/media` or `/data/offlinelog`), and paginated file deletion ([formats and validation](docs/CAPTURE.md)).
- **Native runtime**: core pages use firmware APIs without an extra JS runtime; optional connectivity has separate protocol documentation.

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

1. Use a Release / CI artifact matching this fork’s source commit, or [build](docs/BUILD.md) `chaos-installer-10p-3.101.043.bin`. Upstream binaries do not include this fork’s fixes.
2. Install it through AstroBox's custom-watchface (or third-party app) entry.
3. **Reboot the band** so the module loads at boot.
4. Open Chaos and enable auto-injection / background restore as needed.

Full steps and troubleshooting: **[docs/INSTALL.md](docs/INSTALL.md)**.

## Build

```bash
python3 tools/build.py --firmware 10p-3.101.043  # complete, firmware-isolated build
python3 tools/build.py --firmware 10p-3.101.043 --no-icons --skip-fetch  # rebuild main module; reuse existing icons/fetch
```

The installer is written to `dist/10p-3.101.043/chaos-installer-10p-3.101.043.bin`.
See [symbol/README.md](symbol/README.md) for extraction and exact firmware bindings.

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

Documentation index: [docs/README.md](docs/README.md). See [current performance](docs/PERFORMANCE.md), [development workflow](docs/DEVELOPMENT.md) and [testing](docs/TESTING.md) (Chinese).

This fork builds on the upstream project. The badge shows module source version 1.2.4; packers still use container version 1.2.1. Align these before release as described in [versioning](docs/BUILD.md#版本号规则).

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

“Corona blur” is ported from
[Canopus-Module-Corona](https://github.com/AstralSightStudios/Canopus-Module-Corona), which declares
**AGPL-3.0-only**. The upstream implementation and [original license](supervisor/corona_vendor/LICENSE)
are preserved in `supervisor/corona_vendor/`; provenance, checksums, Chaos integration changes and
redistribution notes are in the [Corona third-party notice](licenses/CANOPUS-MODULE-CORONA-NOTICE.txt).
The complete license is also available in About → Open-source licenses on the device.
The shared blur-region settings apply to both Corona and legacy blur modes. The system blur
page's explanation links to upstream and the [modified Chaos source](https://github.com/qxdho/Chaos-Module/tree/master/supervisor).
Binary redistribution requires the complete corresponding source and build/install scripts for that binary;
an upstream link or license copy alone does not replace the applicable source obligations.

## Acknowledgements

- The Xiaomi Vela / NuttX community and the openvela documentation
- Everyone who validated changes on real hardware and contributed logs or disassembly evidence

<div align="center">

**If this project helps you, consider giving it a Star ⭐**

</div>
