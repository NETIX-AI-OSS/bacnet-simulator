FROM rust:slim-bookworm as builder

WORKDIR /usr/src/app
COPY . .

# Build the application
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=builder /usr/src/app/target/release/bacnet-simulator /usr/local/bin/bacnet-simulator
COPY config.yaml /app/config.yaml

ENV RUST_LOG=info
ENV CONFIG_PATH=/app/config.yaml

# BACnet UDP port
EXPOSE 47808/udp

CMD ["bacnet-simulator"]
