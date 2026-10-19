<div align="center">

# Chaos

**A native application platform for the Xiaomi Smart Band 10 Pro**

A kernel module plus a Lua installer that registers native pages through the firmware's own registration chain.

[简体中文](README.md) | [English](README.en.md)

[![License](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)](LICENSE)
[![Firmware](https://img.shields.io/badge/firmware-3.101.043-green.svg)](#compatibility)
[![Device](https://img.shields.io/badge/device-p67-orange.svg)](#compatibility)
[![Platform](https://img.shields.io/badge/platform-Vela-lightgrey.svg)](#compatibility)

</div>

---

## Overview

Chaos registers a set of Rust-written native pages with the Xiaomi Smart Band 10 Pro firmware so they appear on the home screen as ordinary apps. Icons, taps and transitions are handled by the stock launcher. The module is offline, needs no phone, and does not touch firmware partitions.

It started as a research harness for the 10 Pro and grew into a full device-management UI: apps, watchfaces, fonts, icons, brightness, cache, blur and vibration.

> **Scale**: ~14k lines of Rust, 17 firmware pages, a `chaos_sup.ko` of roughly 175 KB.

## Features

- **App manager**: hide/show, batch actions, exclusions, and an optional "show without reboot" mode (see [App manager](docs/APP-MANAGER.md)).
- **Customization**: fonts, home-screen icon packs, system blur, translucent watchface background.
- **Watchfaces**: switch built-in/market faces, shake-to-rotate, face management.
- **Device tools**: brightness, cache cleanup, time/date, vibration test.
- **No external runtime**: talks only to verified firmware APIs; no JS injection, no network.

## Compatibility

| Item | Requirement |
|---|---|
| Device | Xiaomi Smart Band 10 Pro (`p67` / `M2551B1`) |
| Firmware | **3.101.043** |
| Module | ARM ELF32 `ET_REL`, `thumbv8m.main-none-eabi`, soft-float |
| Installer | Lua container (magic `0x1234A55A`) |

> Firmware addresses are hard-coded. **Do not use this module on 3.101.036 or any other build**; a firmware update requires re-porting the addresses.

## Installation

1. Download the installer `.bin` from [Releases](https://github.com/WenHuaYiYang/Chaos-Module/releases) or a GitHub Actions artifact.
2. Install it through the custom watchface / third-party app entry of a tool such as AstroBox.
3. Reboot the band so the module loads at boot.
4. Open Chaos and enable auto-injection / background restore as needed.

> The first install and every module upgrade need a reboot. Flashing carries risk; back up your data and verify the firmware version first.

## Usage

### App manager

- **Tap an entry**: hide/show that app.
- **Hide all / Show all**: batch operation on the current list; excluded apps are skipped.
- **Exclusions** (page 7): mark apps that should not participate in batch actions; they disappear from page 1.
- **Show without reboot** (switch on page 1):
  - Off (default): hide/show rewrites `/data/apps.json`; **showing takes effect after a reboot**.
  - On: keep the `apps.json` entry and use the registry hidden bit plus a boot-time compensation pass, so **a newly hidden app can be shown immediately**.

See [App manager](docs/APP-MANAGER.md) for the firmware evidence, trade-offs and test checklist.

### Customization

- **Fonts**: switch between delivered fonts; deletion requires two-step confirmation.
- **Home icons**: icon packs and restore-to-stock.
- **System blur / blur zones**: choose regions to blur.
- **Translucent watchface background**: generate and reuse a static background; fixed-image mode needs `background.bin`.

## Building from source

### GitHub Actions (recommended)

[`.github/workflows/build.yml`](.github/workflows/build.yml) runs on push, PR and manual dispatch: it installs a nightly toolchain, runs `sh tools/build_ko.sh`, and uploads `supervisor/chaos_sup.ko`.

### Local build

Requirements:

- Rust nightly (for `-Z build-std`)
- the `rust-src` component
- target `thumbv8m.main-none-eabi`
- Python 3 (post-processing and icon generation)

```bash
# 1. build the module -> supervisor/chaos_sup.ko
sh tools/build_ko.sh

# 2. generate installer assets
python3 tools/build.py
```

What `tools/build_ko.sh` does:

```text
cargo +nightly build --release --target thumbv8m.main-none-eabi
  -Z build-std=core,compiler_builtins
  -C target-cpu=cortex-m33 -C target-feature=-fpregs
        ↓
rust-lld -r --gc-sections -u module_main -u chaos_ctor -T merge_sections.ld
        ↓
fix_ko_layout.py -> verify_chaos_ko.py (zero undefined symbols)
```

> The full downstream packaging pipeline (`scripts/build_chaos_installer_043.py` and friends) is not part of this repository. `tools/container_shell.py` documents the container format; project-specific glue is left to the user.

## Layout

```text
Chaos-Module/
├── supervisor/          kernel module (Rust, no_std)
│   ├── src/
│   │   ├── lib.rs       crate root and module entry
│   │   ├── page.rs      17 pages and PAGE_TABLE
│   │   ├── ui.rs        page framework / rows / event routing
│   │   ├── appmgr.rs    app manager: hide/show, exclusions, modes
│   │   ├── fw_api/      verified firmware API wrappers
│   │   └── ...          watchfaces, fonts, icons, background, settings
│   └── chaos_sup.ko     build artifact (not committed)
├── installer/           Lua installer
├── module/              minimal sample module
├── tools/               build, container, icon generation
└── .github/workflows/   CI
```

## Documentation

- [App manager: hide/show and the no-reboot mode](docs/APP-MANAGER.md)
- Firmware research, address tables and format notes: see the research repository's `docs/`.
- Installer: [`installer/chaos_installer.lua`](installer/chaos_installer.lua)

## Contributing

1. Fork the repository and branch from `master`.
2. Run `sh tools/build_ko.sh` (or wait for CI) after module changes.
3. For changes touching firmware addresses, page counts or registration chains, include evidence in the PR (device logs or disassembly).
4. Make sure no `.ko`, font, commercial asset or secret is committed.

## License

[AGPL-3.0](LICENSE). Redistributed modifications must keep the same license and source offer.

Fonts and third-party assets follow their own licenses; this repository ships no commercial fonts, system icons or firmware images.

## Acknowledgements

- The Xiaomi Vela / NuttX community and the openvela documentation
- Everyone who validated changes on real hardware and contributed logs or disassembly evidence

<div align="center">

**If this project helps you, consider giving it a Star ⭐**

</div>
