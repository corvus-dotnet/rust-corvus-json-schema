# rust-corvus-json-schema

A [Bowtie](https://github.com/bowtie-json-schema/bowtie) test harness for
[corvus-json-schema](https://crates.io/crates/corvus-json-schema), the Rust port of
[Corvus.JsonSchema](https://github.com/corvus-dotnet/Corvus.JsonSchema)'s V5 evaluator.

Its image is published to `ghcr.io/bowtie-json-schema/rust-corvus-json-schema` and run via
`bowtie run -i rust-corvus-json-schema`.

The harness compiles each case's schema with the case's `registry` as the document resolver and validates each
instance. For `annotations` output it evaluates through a verbose results collector and reports each annotation with
its instance location and `#…` keyword location. A panic, or an evaluation beyond the maximum depth, is reported as an
error for that case or instance.

The crate's version is pinned in `Cargo.toml` and `Cargo.lock`, which Dependabot keeps at the latest release.
