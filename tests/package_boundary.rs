#![forbid(unsafe_code)]

const ROOT: &str = include_str!("../src/lib.rs");
const CLI_ROOT: &str = include_str!("../src/cli/mod.rs");
const PUBLIC_API: &str = include_str!("../contracts/api_baselines/radroots_cli.txt");

#[test]
fn implementation_modules_are_private_and_api_is_root_only() {
    assert!(ROOT.contains("mod cli;"));
    assert!(!ROOT.contains("pub mod cli;"));
    assert!(!ROOT.contains("pub use cli::*;"));
    assert!(!CLI_ROOT.contains("::*;"));
    for implementation in [
        "account",
        "basket",
        "diagnostics",
        "farm",
        "health",
        "listing",
        "market",
        "profile",
        "signer",
        "store",
        "sync",
        "trade",
        "transport",
        "validation",
    ] {
        assert!(!PUBLIC_API.contains(&format!("radroots_cli::{implementation}::")));
    }
    for required in [
        "pub struct radroots_cli::TargetCliArgs",
        "pub enum radroots_cli::TargetCommand",
        "pub enum radroots_cli::TargetOutputFormat",
    ] {
        assert!(PUBLIC_API.contains(required), "missing {required}");
    }
    for dependency in ["clap::", "radroots::", "serde_json::"] {
        assert!(!PUBLIC_API.contains(dependency), "leaked {dependency}");
    }
}
