# KIS — PostgreSQL-Workspace (kis-app: Server `kis` und Werkzeug `kis-admin`)
FROM rust:1.99.1-slim-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --locked --release --bins -p kis-app

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r kis && useradd -r -g kis -d /data -s /usr/sbin/nologin kis
WORKDIR /data
COPY --from=builder /build/target/release/kis /build/target/release/kis-admin /usr/local/bin/
COPY config/kis.toml /data/config/kis.toml
COPY static/ /data/static/
RUN mkdir -p /data/logs && chown -R kis:kis /data/logs
USER kis
ENV KIS_SERVER__HOST=0.0.0.0 KIS_LOGGING__FORMAT=json
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s CMD curl -fsS http://127.0.0.1:8080/health || exit 1
ENTRYPOINT ["kis"]
