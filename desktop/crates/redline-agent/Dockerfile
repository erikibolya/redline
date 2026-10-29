# Headless RedLine agent runtime.
#
#   docker build -f desktop/crates/redline-agent/Dockerfile -t redline-agent .
#
# The OpenCode binary and its provider auth are mounted from the host, so this image only carries
# the bridge. Give the container exactly the access the agent should have (repos, docker socket).
FROM rust:1-bookworm AS build
WORKDIR /src
COPY desktop ./desktop
RUN cd desktop && cargo build --release -p redline-agent

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates git ripgrep \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/desktop/target/release/redline-agent /usr/local/bin/redline-agent
ENV REDLINE_AGENT_REPO=/src
WORKDIR /src
ENTRYPOINT ["/usr/local/bin/redline-agent"]
