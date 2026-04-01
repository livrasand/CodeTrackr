# Multi-stage build — cargo-chef for dep caching + distroless runtime

# ── Chef Stage (instala cargo-chef una sola vez) ───────────────────────────────
FROM rust:1.82-slim AS chef
RUN apt-get update && apt-get install -y pkg-config libssl-dev patch && rm -rf /var/lib/apt/lists/*
RUN cargo install cargo-chef --locked
WORKDIR /app

# ── Planner (genera recipe.json con el grafo de deps) ─────────────────────────
FROM chef AS planner
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo chef prepare --recipe-path recipe.json

# ── Builder (compila deps cacheadas + binario final) ──────────────────────────
FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
# Este paso se cachea mientras Cargo.toml/Cargo.lock no cambien
RUN cargo chef cook --release --recipe-path recipe.json

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY migrations ./migrations
RUN cargo build --release

# ── Runtime Stage (imagen mínima ~20MB base) ───────────────────────────────────
FROM gcr.io/distroless/cc-debian12 AS runtime

WORKDIR /app

COPY --from=builder /app/target/release/codetrackr ./codetrackr
COPY --from=builder /app/migrations ./migrations
COPY static ./static

EXPOSE 8080

CMD ["./codetrackr"]
