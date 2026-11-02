# Corona p67 host regression harnesses

The six C files in this directory are verbatim copies of the corresponding
`tests/` files from the user-provided Canopus-Module-Corona source archive:
[AstralSightStudios/Canopus-Module-Corona](https://github.com/AstralSightStudios/Canopus-Module-Corona).
They retain the upstream AGPL-3.0-only license; the full text is in
`../../corona_vendor/LICENSE`. `upstream.sha256` pins both these harnesses and
their vendored implementation dependencies.

From the Chaos repository root, run:

```sh
python3 tools/test_corona_blur.py
```

To additionally compare every vendored source, header, license and harness
byte-for-byte with the original source archive:

```sh
python3 tools/test_corona_blur.py --upstream /path/to/Canopus-Module-Corona-main
```

The runner stages `supervisor/corona_vendor/{src,include}` and these harnesses
in a temporary tree. Thus the harnesses' unchanged `../src/` includes resolve
to **the implementation actually vendored in Chaos**, never a separately
installed Corona checkout. The runner compiles with AddressSanitizer and
UndefinedBehaviorSanitizer, then exercises all six harnesses, battery mask
and charging cases, and the native diagnostic-injection variant.

These are host lifecycle/pixel/safety models. They do not establish device
GPU/cache acceptance, touch/scroll feel, or frame timing. Those require exact
p67 / 3.101.043 hardware validation.

The same command also compiles `../corona_owner_harness.rs` against the current
`corona_tick`, `pending` and `remount` function bodies extracted from
`supervisor/src/background_home.rs`. Its 11 tests exercise the actual Rust
owner arbitration with stubbed native calls: busy enable/disable retries,
deferred retirement, rapid off/on, missing roots, failed detach/release,
default system mode, restoration of legacy modes and sleep/wake scheduling.
The production functions are not duplicated in this fixture.

`../corona_restore_harness.rs` compiles the current legacy surface restore,
event-consumption and pending functions. Its six tests cover temporary missing
roots retaining the restoration ledger and background descriptor, consuming
DELETE/style events safely, restoring valid objects after a retry, and skipping
foreign/detached/unchanged objects. The owner harness also verifies background
clearing happens only after unmount and before releasing the old allocation.

`../corona_abi_harness.c` similarly compiles the current C commit/poll wrapper
bodies with injected native services. It verifies all four paths become
`:blur` together, disabling restores four empty paths, failed commits preserve
the applied state, and busy/reentrant calls do not enter the native owner.
