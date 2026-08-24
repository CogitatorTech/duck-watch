# Build from the workspace root suing `docker build -f backend/Dockerfile .`
FROM rust:1.97-slim-bookworm AS build

RUN apt-get update \
    && apt-get install --no-install-recommends --yes g++ make pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml ./backend/
RUN mkdir -p backend/src && echo "fn main() {}" > backend/src/main.rs
RUN cargo build --release --package backend
RUN rm -rf backend/src

COPY backend/migrations ./backend/migrations
COPY backend/src ./backend/src
RUN touch backend/src/main.rs && cargo build --release --package backend

FROM debian:bookworm-slim

RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home app

COPY --from=build /app/target/release/backend /usr/local/bin/backend
USER app
EXPOSE 8080
ENTRYPOINT [ "backend" ]
