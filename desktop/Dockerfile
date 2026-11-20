# redline server — single static binary, no assets, no shell.
#
# Agents live next to this container (Unraid/NAS/WSL); the Windows app connects to it as a
# thin client. Build: docker build -f desktop/Dockerfile -t redline .

FROM rust:1-bookworm AS build
# `ring` (rustls, via the inbox's HTTPS client) compiles C for the musl target.
RUN apt-get update \
 && apt-get install -y --no-install-recommends musl-tools \
 && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-unknown-linux-musl
WORKDIR /src
COPY desktop ./desktop
# rust-lld + musl target (configured in desktop/.cargo/config.toml) links a static binary,
# so the runtime image needs no libc at all.
RUN cd desktop && cargo build --release --target x86_64-unknown-linux-musl -p redline

FROM scratch
COPY --from=build /src/desktop/target/x86_64-unknown-linux-musl/release/redline /redline
VOLUME ["/data"]
EXPOSE 8788
# Unraid and friends show this as the container status.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD ["/redline", "health"]
# Bound to all interfaces because the human's desktop connects from another machine; the bearer
# token is the guard. Sessions live in the mounted volume.
ENTRYPOINT ["/redline", "serve", "--host", "0.0.0.0", "--data-dir", "/data"]
