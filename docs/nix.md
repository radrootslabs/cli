# Nix

This repository uses Nix as the canonical local development and validation environment.

## Enter The Shell

```bash
nix develop
```

## Validation And Formatting

```bash
nix run .#fmt
nix run .#check
nix run .#test
nix run .#release-acceptance
```

Use `nix run .#check` and `nix run .#test` as the first-line validation pair
from this repo root.

Use `nix run .#release-acceptance` when preparing a production candidate.

Use `nix develop` before running narrower ad hoc cargo commands from this repo
root.
