# Build from the workspace root: docker build -f backend/Dockerfile .
#
# Debian rather than Alpine: the `duckdb` crate compiles libduckdb from C++
# source, which needs a full C++ toolchain and links against libstdc++. The
# musl build of that is fragile enough not to be worth the smaller image.
FROM rust:1.97-slim-bookworm AS build

# g++ builds libduckdb; the rest are what its build script expects to find.
RUN apt-get update \
    && apt-get install --no-install-recommends --yes g++ make pkg-config \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy the manifests first so the dependency layer is cached independently of the source code.
COPY Cargo.toml Cargo.lock ./
COPY backend/Cargo.toml ./backend/
RUN mkdir -p backend/src && echo "fn main() {}" > backend/src/main.rs
RUN cargo build --release --package backend
RUN rm -rf backend/src

COPY backend/migrations ./backend/migrations
COPY backend/src ./backend/src
# Touch the entry point so cargo does not reuse the placeholder build.
RUN touch backend/src/main.rs && cargo build --release --package backend

FROM debian:bookworm-slim

# ca-certificates is required, not optional: the MotherDuck extension is
# downloaded over HTTPS the first time a connection is opened, and libstdc++
# is what the bundled libduckdb links against.
RUN apt-get update \
    && apt-get install --no-install-recommends --yes ca-certificates libstdc++6 \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --uid 10001 --create-home app

COPY --from=build /app/target/release/backend /usr/local/bin/backend
USER app
EXPOSE 8080
ENTRYPOINT [ "backend" ]
