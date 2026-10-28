#!/bin/bash
# Add a Windows code-signing certificate to the repository secrets.
#
#   desktop/scripts/add-signing-cert.sh /path/to/cert.pfx 'password'
#
# After this, the next tagged release is signed automatically: the workflow decodes the PFX,
# points Tauri at it, and signtool signs both redline-app.exe and the NSIS installer.
#
# You need a certificate the OS trusts (Certum Open Source, SignPath, OV/EV from any CA).
# A self-signed certificate produces a signed file that Windows still refuses to trust.
set -euo pipefail

PFX="${1:-}"
PASSWORD="${2:-}"
if [ -z "$PFX" ]; then
  echo "usage: $0 <certificate.pfx> [password]" >&2
  exit 1
fi
[ -f "$PFX" ] || { echo "no such file: $PFX" >&2; exit 1; }

command -v gh >/dev/null || { echo "the GitHub CLI (gh) is required" >&2; exit 1; }
gh auth status >/dev/null 2>&1 || { echo "run 'gh auth login' first" >&2; exit 1; }

repo="$(gh repo view --json nameWithOwner -q .nameWithOwner)"
echo "Adding signing secrets to $repo"

base64 -w0 "$PFX" | gh secret set WINDOWS_CERT_PFX
printf '%s' "$PASSWORD" | gh secret set WINDOWS_CERT_PASSWORD

echo
echo "Done. Verify with:"
echo "  gh secret list | grep WINDOWS_CERT"
echo
echo "Then tag a release; the log should show 'Configure Authenticode signing' running:"
echo "  git tag -a v0.2.6 -m '...' && git push origin v0.2.6"
echo
echo "Check the result with:  signtool verify /pa /v redline_*_x64-setup.exe"
