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
