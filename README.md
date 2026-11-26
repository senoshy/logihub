# LogiHub (Linux)

A native Linux alternative to Logitech G Hub built in Rust, speaking **HID++ 2.0** directly over Linux `/dev/hidraw*` — no background daemon required.

Designed so DPI stages and polling rate changes **actually apply and persist** on Logitech G PRO X SUPERLIGHT (1st gen & 2nd gen) and other HID++ 2.0 Logitech G mice by updating both the live sensor registers (`0x2201`/`0x2202`, `0x8060`/`0x8061`) **and** the active `0x8100` onboard memory profile sector (with CRC-16-CCITT verification).

## Features

- **Direct HID++ 2.0 over `/dev/hidraw*`** — automatic discovery of wired and LIGHTSPEED receiver endpoints (`046D:C547`, `046D:C54D`, wired USB, etc.).
- **DPI & Onboard Stages** — configure up to 5 onboard DPI stages and active DPI with sensor range clamping/snapping and flash persistence.
- **Polling Rate** — 125 / 250 / 500 / 1000 Hz (`0x8060`) and 2000 / 4000 / 8000 Hz (`0x8061` on Superlight 2) with onboard memory persistence.
- **Battery Status** — live percentage and charging state (`0x1004` UnifiedBattery, with `0x1001` / `0x1000` fallbacks).
- **Native Desktop GUI + CLI** — single binary `logihub` (launches GUI by default, or use subcommands for scripting).

## Build, Test & Lint

With Nix flakes:

```bash
nix develop --command cargo test
nix develop --command cargo clippy --all-targets -- -D warnings
nix develop --command cargo build --release
```

Or inside any Rust environment:

```bash
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

## Usage

```bash
# Launch the desktop GUI
logihub

# Inspect status (or --json)
logihub status
logihub status --json

# Check battery
logihub battery

# Set active DPI (applied live + saved to onboard memory)
logihub dpi 800

# Configure all onboard DPI stages and active stage index (0-based)
logihub dpi --stages 400,800,1600,3200 --active 1

# Set polling rate in Hz (applied live + saved to onboard memory)
logihub rate 1000

# Switch between onboard memory mode and host mode
logihub mode onboard
logihub mode host

# Dump all discovered HID++ 2.0 features on the connected mouse
logihub inspect
```

## Permissions (udev)

If opening `/dev/hidraw*` reports permission denied, install [`99-logitech-hidpp.rules`](99-logitech-hidpp.rules):

```bash
sudo cp 99-logitech-hidpp.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
```

On NixOS, add to `services.udev.extraRules`:

```nix
services.udev.extraRules = ''
  KERNEL=="hidraw*", ATTRS{idVendor}=="046d", MODE="0660", TAG+="uaccess"
'';
```
