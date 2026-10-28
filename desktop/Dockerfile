# redline server — single static binary, no assets, no shell.
#
# Agents live next to this container (Unraid/NAS/WSL); the Windows app connects to it as a
# thin client. Build: docker build -f desktop/Dockerfile -t redline .

FROM rust:1-bookworm AS build
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
# Bound to all interfaces because the human's desktop connects from another machine; the bearer
# token is the guard. Sessions live in the mounted volume.
ENTRYPOINT ["/redline", "serve", "--host", "0.0.0.0", "--data-dir", "/data"]
