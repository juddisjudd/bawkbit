# Build from the repository root: docker build -f packaging/docker/Dockerfile -t bawkbit .
FROM oven/bun:1 AS web
WORKDIR /src/web
COPY web/package.json web/bun.lock web/bunfig.toml ./
RUN bun install --frozen-lockfile
COPY web/ ./
RUN bun run build

FROM rust:1-bookworm AS daemon
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY --from=web /src/web/dist web/dist
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked -p bawkbitd

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates tini \
    && rm -rf /var/lib/apt/lists/*
COPY --from=daemon /src/target/release/bawkbitd /usr/local/bin/bawkbitd
ENV BAWKBIT_HOME=/config \
    BAWKBIT_DOWNLOAD_DIR=/downloads \
    BAWKBIT_PORT=51413
VOLUME ["/config", "/downloads"]
EXPOSE 8228 51413/tcp
ENTRYPOINT ["/usr/bin/tini", "--", "bawkbitd"]
