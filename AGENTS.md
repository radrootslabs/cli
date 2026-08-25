# AGENTS.md — Radroots CLI

These instructions apply to the complete standalone `radrootslabs/cli`
repository.

## Repository role

This repository owns the public `radroots_cli` package and the `radroots`
binary. It owns command parsing, invocation-scoped configuration, composition
of public Radroots library clients, stable output envelopes, exit behavior,
and CLI-specific presentation and tests.

It does not own Radroots domain policy, durable service state, signing or
custody internals, relay internals, SDK internals, platform deployment,
publication, promotion, or non-public parent integration behavior. Preserve
those boundaries unless an approved public contract explicitly changes them.

The repository must remain independently cloneable, buildable, testable, and
packageable. Do not depend on private repositories, parent-only contracts or
tools, sibling checkout paths, unpublished local artifacts, absolute host
paths, or an enclosing monorepo layout.

## Authority and published-source boundary

Repository-local machine authority is limited to manifests and locks,
`radroots.lib.source-lock.v1.toml`, the checked-in dependency-policy store, and
explicit machine-readable contracts added under `contracts/**`. Source and
tests are implementation evidence. Native Cargo and repository scripts own
the standalone command surfaces; checked-in Nix material is deferred and
unclaimed through RCLD-RSHR-170. The root `README` is concise public routing
material, not a substitute for a machine contract.

`.env.example` is non-authoritative pre-refactor evidence. It does not describe
current runtime behavior and remains only until its owning later cleanup
checkpoint; do not use it to invent or preserve configuration behavior.

Human implementation specifications, decisions, runbooks, migration history,
qualification records, and execution evidence are parent-owned and do not ship
in this capsule. Never create or consume capsule-local `docs/**`. The parent
documentation is not a standalone command, build, test, package, or release
input.

The physical roots `docs/**`, `.github/**`, and `.act/**` are forbidden,
including symlinks. Run `tools/verify-repository-boundary.sh` in every governed
verification lane. Do not add a capsule-local workflow definition. External
automation may invoke the repository's ordinary forge-agnostic commands, but
it must not be required by an independent clone.

## Dependency and source-lock rules

The public `radroots` dependency must use the canonical
`https://github.com/radrootslabs/lib.git` source, an exact immutable revision,
and the declared exact package version. Keep `Cargo.toml`, `Cargo.lock`, and
`radroots.lib.source-lock.v1.toml` consistent. Never replace it with a path,
branch, floating Git reference, private mirror, or implicit sibling override.

An exact revision is not releasable merely because it exists locally. Before a
downstream pin or release advances, the selected upstream commit must be
publicly reachable and its source-lock evidence must be verified under the
applicable publication authority.

## Change rules

- Inspect the relevant manifest, lock, source lock, implementation, tests, and
  public routing material before changing behavior.
- Make the smallest complete change and keep each checkpoint independently
  reviewable. Do not mix unrelated cleanup or roadmap work into it.
- Preserve a clear separation between argument parsing, configuration loading,
  service-client composition, domain-library calls, and output formatting.
- Prefer typed models, explicit inputs, deterministic behavior, narrow side
  effects, and typed errors for expected failures.
- Do not add compatibility aliases, dual reads, dual writes, fallback behavior,
  or hidden environment/runtime discovery for prototype surfaces being removed
  by the active clean-slate services-hardening sequence. Change those surfaces
  only in their owning implementation checkpoint.
- Keep output schemas, exit codes, help text, configuration examples, source,
  tests, and machine contracts aligned with every user-visible change.
- Keep `unsafe` absent unless an approved contract makes it unavoidable; any
  exception requires a narrow local invariant and dedicated tests.

## Security and operational behavior

Never expose or commit private keys, credentials, tokens, invite codes,
approval proofs, private identifiers, sensitive user data, or sensitive event
content. Examples and fixtures must use unmistakably synthetic values.

Do not log secrets or raw protected material. Keep machine-readable output on
stdout, diagnostics on stderr, and non-success outcomes paired with a stable
structured error and nonzero exit status. Destructive or externally mutating
commands must remain explicit, fail closed, and require their governed
authorization rather than inferring consent from configuration or environment.

Avoid hidden production panics. Bound input, output, network, time, and retry
work where the relevant public contract defines a limit, and preserve
cancellation and failure context without leaking sensitive internals.

## Verification

From an extbuild-enabled checkout, run `cargo extbuild doctor` before the first
mutating build, check, test, package, install, or generated-artifact command,
then route repository-owned commands through `cargo extbuild run -- ...`.
Standalone public verification surfaces are:

```sh
cargo fmt --all --check
cargo check --all-targets --locked
cargo test --all-targets --locked
cargo clippy --all-targets --locked -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --locked
scripts/verify-supply-chain.sh
tools/verify-repository-boundary.sh
```

Use the smallest relevant surface during development and the complete native
set for a production candidate. The supply-chain gate uses exact cargo-deny
0.19.8 and cargo-vet 0.10.2; its checked-in exemptions are visible accepted
review debt, not claims of independent source audits. Nix and OCI remain
deferred and unclaimed through RCLD-RSHR-170. Run `git diff --check` and
inspect the final status and diff before every checkpoint.

Never claim a lane passed unless it ran successfully. Record unavailable or
environment-blocked lanes exactly, and do not treat parent-only automation as a
substitute for standalone repository validation.

## Git and release discipline

Preserve unrelated changes and repository identity. Do not reset, discard,
rewrite, push, tag, sign, publish, deploy, rotate credentials, or advance a
downstream revision without explicit authority for that action. Keep commits
focused and use `<scope>: <imperative summary>` unless a stronger repository
convention applies.
