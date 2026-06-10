# BACnet Simulator

A config-driven BACnet/IP building simulator written in Rust. It emulates many BACnet
devices from a single YAML file and drives their points with realistic behavior —
temperature control loops, occupancy-linked load, scheduled binaries, random walks,
and integrators — so you can test BACnet clients, gateways, and analytics pipelines
without real hardware.

Ships as a **single cross-platform binary**: download, run (or double-click), and it
starts serving BACnet/IP on UDP port **47808**. On first run it writes a full sample
`config.yaml` into the current working directory; later runs reuse that file.

## Features

- Define devices declaratively with reusable **templates** and **instance blocks**.
- Point **profiles** model realistic dynamics: `temp_control`, `occupancy_linked`,
  `binary_schedule`, `random_walk`, `integrator`, `sine`, `multi_state`, and more.
- Building-level **seasonality / occupancy** schedules drive coordinated behavior
  across all devices.
- Serves BACnet/IP over UDP (default port **47808**), responding to Who-Is,
  Read-Property, and Read-Property-Multiple.
- Zero install step beyond the binary — sample config is created automatically.
- **Interactive terminal UI** on interactive terminals — live status, device browser,
  and config generation without leaving the app.

## Quick start

### Download a release binary

Pre-built binaries for Linux, macOS, and Windows are attached to each
[GitHub release](https://github.com/NETIX-AI-OSS/bacnet-simulator/releases)
(tagged `v*`, for example `v0.1.0`).

1. Download the archive for your platform (`linux-x86_64`, `linux-aarch64`,
   `macos-x86_64`, `macos-aarch64`, or `windows-x86_64`).
2. Extract the `bacnet-simulator` executable.
3. Run it from the folder where you want `config.yaml` to live.

**Linux / macOS**

```bash
chmod +x bacnet-simulator
./bacnet-simulator
```

**Windows**

Double-click `bacnet-simulator.exe` in Explorer, or run from a terminal:

```powershell
.\bacnet-simulator.exe
```

On first start the simulator writes `config.yaml` in the **current working directory**
(a comprehensive Marina Heights Tower sample with ~250 devices). Edit that file and
restart to customize the building. If `config.yaml` already exists, it is loaded as-is.

### Run from source

```bash
cargo run --release
```

Same behavior: creates `config.yaml` in the working directory when missing.

Override the config location with:

```bash
CONFIG_PATH=/path/to/config.yaml RUST_LOG=info cargo run --release
```

Force log-only mode (no TUI) for scripts or CI:

```bash
./bacnet-simulator --no-tui
# or: BACNET_SIM_NO_TUI=1 ./bacnet-simulator
```

## Terminal UI

When stdout is a TTY (normal terminal or double-click on Windows/macOS/Linux console),
the simulator opens a **ratatui** dashboard instead of streaming logs.

| Tab | What you see |
| --- | --- |
| **Status** | Building name, config path, BACnet listener, uptime, device/point counts, occupancy and outside temperature, request counters, recent log lines |
| **Devices** | Scrollable device list; `/` to filter; Enter to drill into live point values |
| **Config** | Reset to bundled sample or run a minimal-config wizard |

**Keys:** `Tab` / `←` `→` switch tabs · `↑` `↓` scroll · `Enter` select · `/` filter devices · `Esc` back · `q` quit

**Config changes** are written to `config.yaml` on disk. After saving, press `R` to restart
the process with the new config or `Q` to quit.

## Configuration

Behavior is fully described by `config.yaml`:

- `building` / `seasonality` — name, timezone, and weekday/weekend occupancy curves.
- `id_policy` — base device IDs and per-template addressing blocks.
- `templates` — reusable device definitions and their points/profiles.
- `instances` — how many of each template to materialize.

Device and object IDs are assigned automatically and are guaranteed unique. The
auto-generated sample config is the same as the bundled
[`config.yaml`](config.yaml) in this repository — a complete worked example with
central plant, AHUs, VAVs, meters, lighting, and more.

## Environment variables

| Variable            | Default       | Description                                      |
| ------------------- | ------------- | ------------------------------------------------ |
| `CONFIG_PATH`       | `config.yaml` | Path to the YAML configuration file.             |
| `BACNET_SIM_NO_TUI` | (unset)       | Set to `1` or `true` to disable the terminal UI. |
| `RUST_LOG`          | `info`        | Log level in `--no-tui` mode (`error`/`warn`/`info`/`debug`). |

## Development

```bash
cargo build --release --locked
cargo test --locked
```

### Cutting a release

Push a version tag; CI runs tests, then builds and uploads platform archives to
GitHub Releases:

```bash
git tag v0.1.0
git push origin v0.1.0
```

Artifacts: `bacnet-simulator-<platform>.tar.gz` (Linux, macOS) or
`.zip` (Windows).

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
