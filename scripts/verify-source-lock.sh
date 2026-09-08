#!/usr/bin/env bash
set -euo pipefail

repo_root="$(git rev-parse --show-toplevel)"
cd "$repo_root"
cargo run --locked -p radroots_cli_xtask -- source-lock-check
