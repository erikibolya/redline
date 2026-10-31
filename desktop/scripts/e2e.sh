#!/bin/bash
# Run the browser end-to-end test against a disposable container.
#
#   desktop/scripts/e2e.sh              # against ghcr.io/erikibolya/redline:browser
#   REDLINE_IMAGE=redline:local desktop/scripts/e2e.sh
#   REDLINE_BIN=/path/to/redline desktop/scripts/e2e.sh
#       # mount a freshly built binary over /redline in an existing browser image; no image build
#   E2E_SHOT=/repo/.e2e-shot.png desktop/scripts/e2e.sh   # also save the answered-round screenshot
#
# Needs: docker, and network access for the Node container to fetch puppeteer-core.
set -euo pipefail

IMAGE="${REDLINE_IMAGE:-ghcr.io/erikibolya/redline:browser}"
NAME="${E2E_NAME:-redline-e2e}"
DATA="${E2E_DATA:-/mnt/user/appdata/redline-e2e}"
REPO="$(cd "$(dirname "$0")/../.." && pwd)"

# A prebuilt server binary is all that changes for a UI/Rust change: reuse the browser image for
# Chromium and mount the binary, instead of rebuilding the ~768 MB image for every run.
BIN=()
if [ -n "${REDLINE_BIN:-}" ]; then
  if [ ! -x "$REDLINE_BIN" ]; then
    echo "REDLINE_BIN is not executable: $REDLINE_BIN" >&2
    exit 2
  fi
  REDLINE_BIN="$(cd "$(dirname "$REDLINE_BIN")" && pwd)/$(basename "$REDLINE_BIN")"
  BIN=(-v "$REDLINE_BIN:/redline:ro")
fi

cleanup() { docker rm -f "$NAME" >/dev/null 2>&1 || true; }
trap cleanup EXIT
cleanup
rm -rf "$DATA" && mkdir -p "$DATA"

docker run -d --name "$NAME" --network traefik_network \
  -v "$DATA:/data" -e REDLINE_TOKEN=e2e-token "${BIN[@]}" \
  --entrypoint /bin/sh "$IMAGE" \
  -c '/redline serve --host 0.0.0.0 --data-dir /data & exec chromium --headless=new --no-sandbox --disable-gpu --remote-debugging-port=9222 about:blank' >/dev/null

for _ in $(seq 1 40); do
  if docker exec "$NAME" /redline health --port 8788 >/dev/null 2>&1; then break; fi
  sleep 1
done

docker run --rm --network "container:$NAME" \
  -v "$REPO:/repo" -w /tmp \
  -e REDLINE_BASE=http://127.0.0.1:8788 \
  -e REDLINE_TOKEN=e2e-token \
  -e CHROME_URL=http://127.0.0.1:9222 \
  -e E2E_SHOT="${E2E_SHOT:-}" \
  node:22 sh -c 'npm i --silent puppeteer-core@23 >/dev/null 2>&1 && cp /repo/desktop/scripts/e2e.mjs /tmp/e2e.mjs && node /tmp/e2e.mjs'
