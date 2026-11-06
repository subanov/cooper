#!/usr/bin/env bash
# Runs Playwright inside the official container so browsers, system libraries and fonts
# are the same locally and in CI (screenshots must match the committed baselines).
#
#   scripts/playwright.sh e2e      # against $COOPER_E2E_URL (a running cooper-server)
#   scripts/playwright.sh visual   # Storybook screenshots; add --update-snapshots to refresh
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(node -p 'require("./node_modules/@playwright/test/package.json").version')
image="mcr.microsoft.com/playwright:v${version}-noble"
suite="${1:?usage: playwright.sh e2e|visual [playwright args]}"
shift

case "$suite" in
  e2e) config=playwright.config.ts ;;
  visual) config=playwright.visual.config.ts ;;
  *) echo "unknown suite: $suite" >&2; exit 2 ;;
esac

exec docker run --rm --init --ipc=host --network host \
  --add-host=host.docker.internal:host-gateway \
  --user "$(id -u):$(id -g)" -e HOME=/tmp \
  -e CI="${CI:-}" -e COOPER_E2E_URL="${COOPER_E2E_URL:-http://127.0.0.1:8080}" \
  -v "$PWD:/work" -w /work "$image" \
  node node_modules/@playwright/test/cli.js test --config "$config" "$@"
