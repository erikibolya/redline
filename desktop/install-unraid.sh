#!/bin/bash
# Install/upgrade redline as a Docker container on Unraid (or any Linux host with docker).
#
#   bash install-unraid.sh                 # generate a token and print it
#   REDLINE_TOKEN=xxx bash install-unraid.sh
#
# Re-running it upgrades in place: the data volume is kept.
set -euo pipefail

IMAGE="${REDLINE_IMAGE:-ghcr.io/erikibolya/redline:latest}"
NAME="${REDLINE_NAME:-redline}"
DATA="${REDLINE_DATA:-/mnt/user/appdata/redline}"
PORT="${REDLINE_PORT:-8788}"
# Optional: expose through Traefik on a subdomain. Without this, re-running the script would
# silently recreate the container without the labels and break the HTTPS route.
DOMAIN="${REDLINE_DOMAIN:-}"
TRAEFIK_NETWORK="${REDLINE_TRAEFIK_NETWORK:-traefik_network}"
CERTRESOLVER="${REDLINE_CERTRESOLVER:-cloudflare}"

command -v docker >/dev/null || { echo "docker is not available on this host" >&2; exit 1; }

# Reuse the token from a previous install, so upgrades never break connected apps.
if [ -z "${REDLINE_TOKEN:-}" ] && [ -f "$DATA/token" ]; then
  REDLINE_TOKEN="$(tr -d '\n' < "$DATA/token")"
fi
if [ -z "${REDLINE_TOKEN:-}" ]; then
  REDLINE_TOKEN="$(head -c 32 /dev/urandom | base64 | tr -d '/+=' | head -c 43)"
fi

mkdir -p "$DATA"
# Persist before the container starts, so a re-run (upgrade) reuses it even if the container
# never got to write its own copy.
printf '%s\n' "$REDLINE_TOKEN" > "$DATA/token"
chmod 600 "$DATA/token"

echo "==> pulling $IMAGE"
docker pull "$IMAGE"

if docker ps -a --format '{{.Names}}' | grep -qx "$NAME"; then
  echo "==> replacing existing container $NAME (data in $DATA is kept)"
  docker rm -f "$NAME" >/dev/null
fi

# With a Traefik route the container needs no published port; that keeps exactly one front
# door (HTTPS + token) instead of an extra plaintext one.
PORTS=(-p "$PORT:8788")
if [ -n "$DOMAIN" ] && [ "${REDLINE_PUBLISH_PORT:-0}" != "1" ]; then
  PORTS=()
  echo "==> not publishing $PORT (Traefik only); set REDLINE_PUBLISH_PORT=1 to keep it"
fi

echo "==> starting $NAME on port $PORT"
LABELS=()
NETWORK=()
if [ -n "$DOMAIN" ]; then
  NETWORK=(--network "$TRAEFIK_NETWORK")
  LABELS=(
    --label traefik.enable=true
    --label "traefik.docker.network=$TRAEFIK_NETWORK"
    --label "traefik.http.routers.$NAME.entrypoints=websecure"
    --label "traefik.http.routers.$NAME.rule=Host(\`$DOMAIN\`)"
    --label "traefik.http.routers.$NAME.tls=true"
    --label "traefik.http.routers.$NAME.tls.certresolver=$CERTRESOLVER"
    --label "traefik.http.services.$NAME.loadbalancer.server.port=8788"
  )
fi
docker run -d \
  --name "$NAME" \
  --restart unless-stopped \
  "${NETWORK[@]}" \
  "${PORTS[@]}" \
  -v "$DATA:/data" \
  -e "REDLINE_TOKEN=$REDLINE_TOKEN" \
  "${LABELS[@]}" \
  "$IMAGE" >/dev/null

# Wait for the health endpoint before declaring success.
for _ in $(seq 1 30); do
  if curl -fsS "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then break; fi
  sleep 1
done

IP="$(ip route get 1.1.1.1 2>/dev/null | awk '{print $7; exit}')"
[ -n "$IP" ] || IP="$(hostname -I 2>/dev/null | awk '{print $1}')"
echo
echo "redline is running."
echo "  session list : http://${IP:-<this-host>}:$PORT/?t=$REDLINE_TOKEN"
[ -n "$DOMAIN" ] && echo "  via traefik  : https://$DOMAIN/?t=$REDLINE_TOKEN"
echo "  token        : $REDLINE_TOKEN"
echo "  sessions     : $DATA/sessions"
echo
echo "Agent config (on this host):"
cat <<JSON
{
  "mcpServers": {
    "redline": {
      "type": "http",
      "url": "http://127.0.0.1:$PORT/mcp",
      "headers": { "Authorization": "Bearer $REDLINE_TOKEN" }
    }
  }
}
JSON
echo
echo "Windows app: tray -> Connect to a server… -> http://<this-host>:$PORT"
