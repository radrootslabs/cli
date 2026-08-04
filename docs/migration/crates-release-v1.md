# Radroots crates release v1 migration

This release moves the CLI onto the final, versioned Radroots crate graph. It
is intentionally breaking: retired compatibility imports, parallel generic
engines, and implicit workspace paths are no longer supported.

## Dependency migration

- Depend on `radroots = "=0.1.0-alpha"` for the supported ordinary application API.
- Keep exact versions and a committed lockfile during the coordinated alpha.
- Do not add sibling `path` dependencies to a production manifest.
- Local contributors may use the explicit ignored patch file documented in
  [`../engineering/local-overrides.md`](../engineering/local-overrides.md).
- Release checks must resolve normalized `.crate` archives from a registry.

The public crate family remains split across the existing `oss/lib` and
`oss/sdk` repositories. This migration does not create a replacement repository
or merge those repositories.

## Runtime migration

The CLI now composes only the canonical `radroots` facade. CLI code owns
command parsing and terminal or structured presentation; storage, signing,
transport, and sync behavior remain with the final crates. It must not
reintroduce a generic relay pagination loop, ingest reducer, outbox engine, or
signer protocol.

The old environment and TOML groups are rejected instead of silently mapped.
Start from `.env.example`, inspect the resolved profile, and address health
actions in order:

```sh
radroots profile inspect --format json
radroots health inspect --format json
```

Other resource commands remain parseable but fail closed with
`unsupported_operation` until a future release enables their canonical SDK
orchestration. This is an intentional breaking change from the private runtime.

Automation should use `--no-input`, explicit online/offline policy, and stable
idempotency and correlation identifiers for writes. Retired commands and flags
fail during parsing; use the resource-oriented command tree shown by
`radroots --help`.

## Compatibility policy

No compatibility import or legacy package name is retained in the supported
surface. A downstream is compatible only when it builds with the exact registry
artifacts, consumes the current output envelope and runtime contract, and does
not require a private or sibling source checkout.
