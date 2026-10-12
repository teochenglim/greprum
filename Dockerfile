# syntax=docker/dockerfile:1.7
#
# GrepRUM image: one binary, non-root, spool on a volume.
#   docker build -t greprum --build-arg VERSION=v0.7.0 .
#   docker run -p 8080:8080 -v greprum-data:/data -v ./greprum.toml:/etc/greprum/greprum.toml:ro greprum

# ---- tracker: node builds + minifies t.js, size budget enforced ---------------
FROM node:26-alpine AS tracker
WORKDIR /src
COPY tracker ./tracker
RUN cd tracker && npm ci --no-audit --no-fund \
 && cd .. && node tracker/build.mjs && node tracker/size.mjs

# ---- binary ---------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY src ./src
COPY --from=tracker /src/tracker/dist ./tracker/dist
ARG VERSION=dev
ENV GREPRUM_VERSION=${VERSION}
RUN cargo build --release --locked \
 && mkdir -p /out/data/spool \
 && cp target/release/greprum /out/greprum

# ---- runtime: distroless, uid 65532 -------------------------------------------
FROM gcr.io/distroless/cc-debian12:nonroot
ARG VERSION=dev
LABEL org.opencontainers.image.title="GrepRUM" \
      org.opencontainers.image.description="Product analytics and RUM in one Rust binary, on GreptimeDB" \
      org.opencontainers.image.source="https://github.com/teochenglim/greprum" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="${VERSION}"
COPY --from=build /out/greprum /usr/local/bin/greprum
COPY --chown=65532:65532 --from=build /out/data /data
COPY docker/greprum.toml /etc/greprum/greprum.toml
ENV GREPRUM_SPOOL_DIR=/data/spool \
    GREPRUM_LISTEN=0.0.0.0:8080
VOLUME ["/data"]
EXPOSE 8080
USER nonroot
HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD ["/usr/local/bin/greprum", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/greprum"]
CMD ["serve", "--config", "/etc/greprum/greprum.toml"]
