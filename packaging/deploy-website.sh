#!/bin/sh
# Deploys website/ (including website/downloads/, filled by packaging/macos/build-app.sh)
# to the Cloudflare Pages project "stickmouse".
#
# First run: authenticate with `npx wrangler login`. If the project does not exist yet,
# wrangler offers to create it. The custom domain stickmouse.app is attached in the
# Cloudflare dashboard under Workers & Pages > stickmouse > Custom domains.
set -eu
cd "$(dirname "$0")/.."

if [ ! -f website/downloads/Stickmouse.dmg ]; then
  echo "warning: website/downloads/Stickmouse.dmg is missing; run packaging/macos/build-app.sh first" >&2
fi

npx wrangler pages deploy website --project-name stickmouse --branch main
