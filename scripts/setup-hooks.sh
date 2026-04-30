#!/usr/bin/env bash
# Install project git hooks by pointing git at .githooks/.
set -euo pipefail
git config core.hooksPath .githooks
echo "Git hooks installed from .githooks/"
