#!/bin/sh
# Deploys website/ (including website/downloads/, filled by packaging/macos/build-app.sh)
# to the Cloudflare Pages project "stickmouse".
#
# First run: authenticate with `npx wrangler login`. If the project does not exist yet,
# create it once with `npx wrangler pages project create stickmouse --production-branch main --force`.
# The site is served at https://stickmouse.pages.dev, the address the app checks for updates.
set -eu
cd "$(dirname "$0")/.."

if [ ! -f website/downloads/Stickmouse.dmg ]; then
  echo "warning: website/downloads/Stickmouse.dmg is missing; run packaging/macos/build-app.sh first" >&2
fi

npx wrangler pages deploy website --project-name stickmouse --branch main
