# Local Radroots package overrides

The committed CLI manifest contains only exact versioned Radroots package
requirements. Production, release, and packaged-consumer checks must not use a
sibling source checkout.

Before the coordinated registry publication, local development may use an
explicit Cargo config outside the release graph. Create
`.cargo/local-paths.toml` with `[patch.crates-io]` entries pointing to the
required `oss/lib` and `oss/sdk` packages, then pass it explicitly:

```sh
cargo --config .cargo/local-paths.toml check --all-targets
cargo --config .cargo/local-paths.toml test --all-targets
```

The local config is ignored and must never be committed. Do not use it for
package-artifact, release-graph, or publication qualification; those lanes must
resolve the exact versions from the configured registry.

The packaged canary must perform all of the following from disposable paths:

1. package the required `oss/lib` crates in Cargo-resolved order;
2. package `radroots_runtime_contract_v1` and `radroots_sdk` from `oss/sdk`;
3. index the resulting `.crate` archives and their checksums in a local registry;
4. package `radroots_cli` with crates.io replaced by that registry;
5. extract the CLI archive and run locked check, test, and help smoke commands.

Passing a source-tree check with `.cargo/local-paths.toml` is not evidence that
the packaged canary is green. The extracted package's Cargo metadata must show
registry sources for every Radroots dependency and no `path` or `git` source.
