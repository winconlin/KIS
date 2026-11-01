# KIS — Multi-Stage Build
#
# Modular: Feature-Flags steuern, welche Module kompiliert werden.
# Standard: alle Module aktiv.
#
# Custom Build: docker build --build-arg FEATURES="patients,fhir,medico" .

FROM rust:1.79-slim AS builder

ARG FEATURES="patients,doctors,appointments,medications,fhir,medico,ui"
WORKDIR /build

# Build-Dependencies
RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*

COPY . .

# Nur aktiviere Module kompilieren (deaktivierte = 0 Bytes)
RUN cargo build --release --no-default-features --features "${FEATURES}"

# --- Runtime ---
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r kis && useradd -r -g kis -d /data -s /sbin/nologin kis

WORKDIR /data

COPY --from=builder /build/target/release/kis /usr/local/bin/kis
COPY config/ /data/config/
COPY migrations/ /data/migrations/

RUN mkdir -p /data/data /data/logs /data/exports && chown -R kis:kis /data

USER kis

ENV RUST_LOG="kis=info"

EXPOSE 8080 6667 104

ENTRYPOINT ["kis"]
