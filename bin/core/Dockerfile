## Quomodo core image: the core binary, the km cli, deno for Actions, and the built UI.
## Build from the repo root:
##   podman build -f bin/core/Dockerfile --build-arg VERSION=1.0.0 -t quomodo .

FROM rust:1.98.1-bookworm AS binaries
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
ARG VERSION="0.0.0"
RUN cargo set-version -p komodo_core ${VERSION}
RUN cargo build -p komodo_core --release && \
  cargo build -p komodo_cli --release && \
  cargo strip

# Build UI
FROM node:22.12-alpine AS ui-builder
WORKDIR /builder
COPY ./ui ./ui
COPY ./client/core/ts ./client
RUN cd client && yarn && yarn build && yarn link
RUN cd ui && yarn link komodo_client && yarn && yarn build

FROM debian:trixie-slim

COPY ./bin/core/starship.toml /starship.toml
COPY ./bin/core/debian-deps.sh .
RUN sh ./debian-deps.sh && rm ./debian-deps.sh

COPY ./config/core.config.toml /config/.default.config.toml
COPY --from=ui-builder /builder/ui/dist /app/ui
COPY --from=binaries /builder/target/release/quomodo /usr/local/bin/quomodo
COPY --from=binaries /builder/target/release/km /usr/local/bin/km
COPY --from=denoland/deno:bin /deno /usr/local/bin/deno

# Set $DENO_DIR and preload external Deno deps
ENV DENO_DIR=/action-cache/deno
RUN mkdir /action-cache && \
	cd /action-cache && \
	deno install jsr:@std/yaml jsr:@std/toml

COPY ./bin/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

# Hint at the port
EXPOSE 9120

ENV KOMODO_CLI_CONFIG_PATHS="/config"
# This ensures any `komodo.cli.*` takes precedence over the Core `/config/*config.*`
ENV KOMODO_CLI_CONFIG_KEYWORDS="*config.*,*komodo.cli*.*"

ENTRYPOINT [ "entrypoint.sh" ]
CMD [ "quomodo" ]

LABEL org.opencontainers.image.source="https://github.com/LoganRupe/quomodo"
LABEL org.opencontainers.image.description="Quomodo core"
LABEL org.opencontainers.image.licenses="GPL-3.0"
