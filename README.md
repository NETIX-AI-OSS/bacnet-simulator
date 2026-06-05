# BACnet Simulator

A config-driven BACnet/IP building simulator written in Rust. It emulates many BACnet
devices from a single YAML file and drives their points with realistic behavior —
temperature control loops, occupancy-linked load, scheduled binaries, random walks,
and integrators — so you can test BACnet clients, gateways, and analytics pipelines
without real hardware.

## Features

- Define devices declaratively with reusable **templates** and **instance blocks**.
- Point **profiles** model realistic dynamics: `temp_control`, `occupancy_linked`,
  `binary_schedule`, `random_walk`, `integrator`, `sine`, `multi_state`, and more.
- Building-level **seasonality / occupancy** schedules drive coordinated behavior
  across all devices.
- Serves BACnet/IP over UDP (default port **47808**), responding to Who-Is,
  Read-Property, and Read-Property-Multiple.

## Quick start

### Run from source

```bash
cargo run --release
```

By default it loads `config.yaml` from the working directory. Override with:

```bash
CONFIG_PATH=/path/to/config.yaml RUST_LOG=info cargo run --release
```

### Run with Docker

Pull the published multi-arch image (linux/amd64, linux/arm64) from GHCR:

```bash
docker pull ghcr.io/netix-ai-oss/bacnet-simulator:latest
```

BACnet/IP relies on UDP broadcast, so host networking is recommended:

```bash
docker run --rm --network host \
  -v "$(pwd)/config.yaml:/app/config.yaml:ro" \
  ghcr.io/netix-ai-oss/bacnet-simulator:latest
```

Or with the bundled compose file:

```bash
docker compose up --build
```

## Configuration

Behavior is fully described by `config.yaml`:

- `building` / `seasonality` — name, timezone, and weekday/weekend occupancy curves.
- `id_policy` — base device IDs and per-template addressing blocks.
- `templates` — reusable device definitions and their points/profiles.
- `instances` — how many of each template to materialize.

Device and object IDs are assigned automatically and are guaranteed unique. See the
provided `config.yaml` for a complete worked example.

## Environment variables

| Variable      | Default       | Description                          |
| ------------- | ------------- | ------------------------------------ |
| `CONFIG_PATH` | `config.yaml` | Path to the YAML configuration file. |
| `RUST_LOG`    | `info`        | Log level (`error`/`warn`/`info`/`debug`). |

## Development

```bash
cargo build --release --locked
cargo test --locked
```

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
