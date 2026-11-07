<div align="center">

# gctl

**Full control of your Glorious Core v1 wireless mouse, straight from Linux.**

One binary, three faces: a scriptable CLI, a native GUI that follows your desktop theme, and a protocol library. Reads and writes every setting the Glorious Core v1 protocol knows, over hidraw, with no daemon.

[![ci](https://github.com/uwuclxdy/gctl/actions/workflows/ci.yml/badge.svg)](https://github.com/uwuclxdy/gctl/actions/workflows/ci.yml)
[![coverage](https://cov.uwuclxdy.dev/badges/uwuclxdy/gctl/coverage.svg)](https://cov.uwuclxdy.dev)
[![ratio](https://cov.uwuclxdy.dev/badges/uwuclxdy/gctl/ratio.svg)](https://cov.uwuclxdy.dev)

</div>

## Why

- **Glorious Core parity.** DPI stages with per-stage colours, the active stage, polling rate, lift-off distance, debounce, sleep delay, the lighting effects with speed and brightness, button binds, scroll inversion, and the three onboard profiles: all of it settable, all of it stored on the mouse.
- **Honest about write-only settings.** The protocol has no read-back for effect, brightness, DPI colours, binds or scroll direction. gctl shows the last value it wrote, marked `(last set by gctl)`, or `unknown` before the first write, instead of guessing.
- **CLI first.** `gctl get` and `gctl set` are stable and scriptable; the GUI is the same grammar with pixels on it.
- **Profiles travel.** Export one profile plus the device-wide settings to a TOML file and import it anywhere; the whole file is validated before the first write.
- **Reads what can be read.** Battery percentage and firmware version come straight from the mouse, and `gctl get battery` tells you when the mouse is asleep instead of failing.

## How it works

| piece | what it does |
|---|---|
| `gctl-proto` | the wire protocol library: 65-byte HID feature reports on interface 0x02, encoder and decoder pinned by tests against the protocol mxw mapped |
| the CLI | the value grammar, the write-only memory, export and import |
| the GUI | the same grammar, edits collected per section and written with Apply |

Every process holds one file lock for the length of a device transaction, so two gctl processes never interleave reports. Opening the hidraw node sends nothing.

## Install

> [!IMPORTANT]
> The mouse's hidraw node is root-only until a udev rule grants it to your desktop session. Install the rule first, then replug the mouse (or run the trigger line below).

```sh
sudo cp 70-gctl.rules /etc/udev/rules.d/
sudo udevadm control --reload
sudo udevadm trigger
```

Build from source:

```sh
cargo install --git https://github.com/uwuclxdy/gctl
```

The build needs `libudev` and `pkg-config` (the HID crate links libudev).

## Usage

```sh
gctl                                open the GUI
gctl get                            every setting of the active profile, one line each
gctl get polling-rate               the bare value: 1000
gctl set dpi 400 800 1600           the stage list (1-6 values, 100-19000)
gctl set dpi-stage 2                the active stage
gctl set dpi-colors ff0000 00ff00   one colour per configured stage
gctl set polling-rate 1000          125 250 500 1000
gctl set lift-off 1                 1 or 2 mm
gctl set debounce 10                0-16 ms
gctl set sleep 2m                   also 90s, 1m30s, never
gctl set effect wave --rate 40      8 effects plus off
gctl set brightness 200 50          wired wireless, 0-255
gctl set bind back key:ctrl+KeyA
gctl set scroll invert
gctl export profile1.toml           one profile plus the device-wide settings
gctl import profile1.toml           asks before overwriting device-wide settings
```

`--profile 1` to `--profile 3` scopes the per-profile settings; without it gctl reads the active profile once. Exit codes: 0 success, 1 runtime failure, 2 usage error.

## Supported mice

Every Glorious Core v1 wireless model the wire protocol knows is handled (12 USB PIDs, wired and wireless). Only the **Model O Wireless** is tested against real hardware; every other model is labelled untested in the GUI and on every CLI command.

<details>
<summary>The model list</summary>

Model O Wireless, Model O- Wireless, Model D Wireless, Model D- Wireless, Model O PRO, Model D2 PRO, Series 1 PRO (wired and wireless where the protocol defines both), plus one unidentified wired model.

</details>

## Known limits

> [!IMPORTANT]
> Do not run mxw, Glorious Core (including in a VM), or any other tool against the mouse while gctl talks to it. gctl's lock serializes gctl processes only, and replies are matched by command id alone.

- Settings the mouse cannot report show the last value gctl set. Another program or a factory reset may have changed them since.
- The Linux input stack (libinput) debounces mouse buttons on top of the firmware's debounce: it drops a release followed by a re-press within 25 ms once it detects bouncing. A debounce of 0 ms in gctl therefore still lands above libinput's filter on affected devices.
- No macros, no shortcut daemon, no firmware update, no dongle pairing. Shortcut actions need a host daemon reading the device's interrupt reports, which hidapi cannot do on Linux.

## FAQ

### Does gctl support the Model O 2 or Model I 2?
No. Newer Glorious mice use USB vendor 0x093A and a different protocol that nobody has mapped publicly yet.

### Why does gctl print `(last set by gctl)`?
Those settings have no read-back in the protocol. The mark means this is what gctl last wrote, not what the mouse holds now.

### Does gctl run on Windows or macOS?
It targets Linux over hidraw and is untested anywhere else.

## Comparison

| | gctl | mxw | Glorious Core |
|---|---|---|---|
| platform | Linux | Linux | Windows |
| interface | CLI + GUI | CLI | GUI |
| export and import profiles | TOML file | - | - |
| runs from scripts | yes | yes | no |

mxw is the project that mapped the wire protocol; gctl credits it and exists because of it.

## Development

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo nextest run --all-features
```

CI runs the same checks on every push, plus a dependency policy (cargo-deny, cargo-audit) and an MSRV check.

## License

GPL-3.0-only. The protocol facts and parts of the configuration encoders come from [mxw](https://github.com/dkbednarczyk/mxw) (GPL-3.0), whose pre-boundary mow portions carry MIT (`LICENSE-MIT-mow` ships the notice). The embedded fonts Onest and JetBrains Mono ship under the SIL Open Font License (`LICENSE-OFL-Onest`, `LICENSE-OFL-JetBrains-Mono`).
