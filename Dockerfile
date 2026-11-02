# Build des aktiven SQLite-Prototyps; PostgreSQL-Module sind noch nicht eingebunden.
FROM rust:1.99.1-slim-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --locked --release --bins

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl sqlite3 \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r kis && useradd -r -g kis -d /data -s /usr/sbin/nologin kis
WORKDIR /data
COPY --from=builder /build/target/release/kis /build/target/release/kis-admin /usr/local/bin/
COPY config/ /data/config/
RUN mkdir -p /data/data /data/logs && chown -R kis:kis /data
USER kis
ENV RUST_LOG=kis=info KIS_SERVER__HOST=0.0.0.0
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:8080/api/health || exit 1
ENTRYPOINT ["kis"]
