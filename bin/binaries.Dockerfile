## Builds the Quomodo core, agent and km binaries for one architecture.
## OpenSSL is vendored, so the binaries do not depend on the host libssl.

FROM rust:1.98.0-bookworm AS builder
RUN cargo install cargo-strip cargo-edit

WORKDIR /builder
COPY Cargo.toml Cargo.lock ./
COPY ./lib ./lib
COPY ./client/core/rs ./client/core/rs
COPY ./client/periphery ./client/periphery
COPY ./bin/core ./bin/core
COPY ./bin/periphery ./bin/periphery
COPY ./bin/cli ./bin/cli
COPY ./xtask ./xtask

# Set Version
ARG VERSION="0.0.0"
RUN cargo set-version -p komodo_core ${VERSION}

# Compile bin
RUN \
  cargo build -p komodo_core --release && \
  cargo build -p komodo_periphery --release && \
  cargo build -p komodo_cli --release && \
  cargo strip

# Copy just the binaries to scratch image
FROM scratch

COPY --from=builder /builder/target/release/quomodo /quomodo
COPY --from=builder /builder/target/release/quomodo-agent /quomodo-agent
COPY --from=builder /builder/target/release/km /km

LABEL org.opencontainers.image.description="Quomodo binaries"
LABEL org.opencontainers.image.licenses="GPL-3.0"
