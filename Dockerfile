# syntax=docker/dockerfile:1

# ---- Build UI ----
FROM node:22-bookworm-slim AS ui-builder
WORKDIR /app/ui

COPY ui/package.json ui/package-lock.json ./
RUN npm ci

COPY ui/ ./
RUN npm run build

# ---- Build binary ----
FROM rust:1-bookworm AS builder
WORKDIR /app

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        cmake \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY build.rs ./

# Create a dummy main so we can cache dependencies
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release --locked 2>/dev/null || true

# Remove dummy build artifacts and overlay real source
RUN rm -rf src
COPY src/ ./src/
COPY --from=ui-builder /app/ui/dist ./ui/dist

RUN touch src/main.rs && cargo build --release --locked

# ---- Runtime ----
FROM debian:bookworm-slim AS runtime

RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /var/lib/rustproxy --create-home rustproxy \
    && mkdir -p /etc/rustproxy/cert.d \
    && chown -R rustproxy:rustproxy /etc/rustproxy

COPY --from=builder /app/target/release/rustproxy /usr/local/bin/rustproxy

USER rustproxy
ENV RUSTPROXY_DB=/var/lib/rustproxy/rustproxy.db
WORKDIR /var/lib/rustproxy
VOLUME ["/var/lib/rustproxy", "/etc/rustproxy/cert.d"]
EXPOSE 3000 80 443

ENTRYPOINT ["rustproxy"]
CMD ["serve"]
