# cli - code directives

- this repo defines `radroots_cli`, the Radroots command-line interface; the primary binary is `radroots`
- treat this repo root as the source of truth for source, user-facing command behavior, repo-local validation, docs, and release-candidate readiness
- do not make this repo responsible for platform-wide signed artifacts, builder selection, publication, promotion, deployment transport, relay internals, signer internals, or SDK internals unless the public CLI contract explicitly changes
- prefer the smallest coherent change that fully addresses the request; do not mix unrelated cleanup, speculative refactors, compatibility scaffolding, or roadmap work into the same change
- inspect the relevant implementation, tests, manifests, and docs before changing behavior; read `README.md` and `Cargo.toml` before broad edits
- do not depend on private repositories, unpublished artifacts, local machine layouts, absolute paths, or internal monorepo context
- keep public docs, manifests, tests, generated artifacts, and contract surfaces aligned with behavior changes
- preserve clear boundaries between argument parsing, configuration loading, service clients, domain logic, and output formatting
- prefer explicit typed models, deterministic behavior, narrow side effects, and direct service boundaries over stringly or implicit behavior
- avoid hidden production panics; use typed errors for expected failure modes
- avoid `unsafe` unless it is strictly necessary, locally justified, and documented with nearby invariants
- do not expose secrets, private keys, credentials, tokens, invite codes, private identifiers, sensitive user data, or sensitive event content in code, logs, tests, fixtures, docs, or examples
- use checked-in, repo-owned validation first; run the smallest documented validation that credibly covers the change, and use release acceptance validation for production candidates
- if validation cannot run, report exactly what was skipped and why; never claim validation passed unless it actually ran
- keep commits focused and reviewable, using `<scope>: <imperative summary>` unless a repo convention overrides it
