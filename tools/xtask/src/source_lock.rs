use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_TEXT: u64 = 2 * 1024 * 1024;
const MAX_METADATA: u64 = 4 * MAX_TEXT;
const FIELDS: [&str; 9] = [
    "schema",
    "repository",
    "revision",
    "architecture",
    "workspace_catalog_sha256",
    "version",
    "source_archive_sha256",
    "lockfile",
    "lockfile_sha256",
];

pub fn run() -> Result<(), ()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .ok_or(())?;
    let lock = read(root.join("radroots.lib.source-lock.v1.toml").as_path())?;
    let cargo_lock = read(root.join("Cargo.lock").as_path())?;
    let metadata = metadata(root)?;
    validate(&lock, &cargo_lock, &metadata)?;
    println!("source lock ok: exact digest, public revision and dependency agreement");
    Ok(())
}

fn read(path: &Path) -> Result<Vec<u8>, ()> {
    let file_type = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !file_type.is_file() || file_type.len() > MAX_TEXT {
        return Err(());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| ())?
        .take(MAX_TEXT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err(());
    }
    Ok(bytes)
}

fn metadata(root: &Path) -> Result<Value, ()> {
    let mut child = Command::new("cargo")
        .args(["metadata", "--locked", "--offline", "--format-version", "1"])
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| ())?;
    let mut bytes = Vec::new();
    let read = child
        .stdout
        .take()
        .ok_or(())?
        .take(MAX_METADATA + 1)
        .read_to_end(&mut bytes);
    if read.is_err() || bytes.len() as u64 > MAX_METADATA {
        let _ = child.kill();
        let _ = child.wait();
        return Err(());
    }
    if !child.wait().map_err(|_| ())?.success() {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}

fn fields(bytes: &[u8]) -> Result<BTreeMap<&str, &str>, ()> {
    let text = std::str::from_utf8(bytes).map_err(|_| ())?;
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, quoted) = line.split_once(" = ").ok_or(())?;
        let value = quoted
            .strip_prefix('"')
            .and_then(|value| value.strip_suffix('"'))
            .ok_or(())?;
        if !FIELDS.contains(&key)
            || value.is_empty()
            || value
                .bytes()
                .any(|byte| !byte.is_ascii_graphic() || matches!(byte, b'"' | b'\\'))
            || fields.insert(key, value).is_some()
        {
            return Err(());
        }
    }
    if fields.len() != FIELDS.len() {
        return Err(());
    }
    Ok(fields)
}

fn validate(lock: &[u8], cargo_lock: &[u8], metadata: &Value) -> Result<(), ()> {
    let fields = fields(lock)?;
    for (key, expected) in [
        ("schema", "radroots.lib.source-lock.v1"),
        ("repository", "https://github.com/radrootslabs/lib"),
        ("architecture", "radroots.crates.release.v2"),
        ("lockfile", "Cargo.lock"),
    ] {
        if fields[key] != expected {
            return Err(());
        }
    }
    for (key, length) in [
        ("revision", 40),
        ("workspace_catalog_sha256", 64),
        ("source_archive_sha256", 64),
        ("lockfile_sha256", 64),
    ] {
        if fields[key].len() != length
            || !fields[key]
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(());
        }
    }
    if fields["lockfile_sha256"] != hex::encode(Sha256::digest(cargo_lock)) {
        return Err(());
    }
    let source = format!(
        "git+https://github.com/radrootslabs/lib.git?rev={}",
        fields["revision"]
    );
    let resolved = format!("{source}#{}", fields["revision"]);
    let packages = metadata["packages"].as_array().ok_or(())?;
    let cli = packages
        .iter()
        .find(|package| package["name"] == "radroots_cli")
        .ok_or(())?;
    let dependencies = cli["dependencies"].as_array().ok_or(())?;
    let facade = dependencies
        .iter()
        .find(|dependency| dependency["name"] == "radroots")
        .ok_or(())?;
    if facade["source"] != source
        || facade["req"] != format!("={}", fields["version"])
        || !facade["rename"].is_null()
    {
        return Err(());
    }
    let mut shared_count = 0;
    for package in packages {
        let name = package["name"].as_str().ok_or(())?;
        if (name == "radroots" || name.starts_with("radroots_"))
            && !matches!(name, "radroots_cli" | "radroots_cli_xtask")
        {
            if package["source"] != resolved || package["version"] != fields["version"] {
                return Err(());
            }
            shared_count += 1;
        }
    }
    if shared_count == 0 {
        return Err(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> (String, Value) {
        let lock = include_str!("../../../radroots.lib.source-lock.v1.toml");
        let lock = lock.replace(
            fields(lock.as_bytes()).expect("canonical lock")["lockfile_sha256"],
            &hex::encode(Sha256::digest(b"synthetic cargo lock")),
        );
        let revision = fields(lock.as_bytes()).expect("canonical lock")["revision"];
        let source = format!("git+https://github.com/radrootslabs/lib.git?rev={revision}");
        let metadata = json!({"packages": [
            {"name": "radroots_cli", "dependencies": [{"name": "radroots", "source": source, "req": "=0.1.0-alpha", "rename": null}]},
            {"name": "radroots", "source": format!("{source}#{revision}"), "version": "0.1.0-alpha"}
        ]});
        (lock, metadata)
    }

    #[test]
    fn exact_lock_and_dependency_inputs_pass() {
        let (lock, metadata) = fixture();
        assert!(validate(lock.as_bytes(), b"synthetic cargo lock", &metadata).is_ok());
    }

    #[test]
    fn stale_lock_bytes_fail_even_when_revision_agrees() {
        let (lock, metadata) = fixture();
        assert!(validate(lock.as_bytes(), b"changed cargo lock", &metadata).is_err());
    }

    #[test]
    fn local_or_mismatched_resolved_shared_sources_fail() {
        let (lock, metadata) = fixture();
        for source in [
            Value::Null,
            json!("git+https://github.com/radrootslabs/lib.git?branch=master"),
        ] {
            let mut changed = metadata.clone();
            changed["packages"][1]["source"] = source;
            assert!(validate(lock.as_bytes(), b"synthetic cargo lock", &changed).is_err());
        }
    }

    #[test]
    fn manifest_revision_or_version_drift_fails() {
        let (lock, metadata) = fixture();
        for (field, value) in [
            ("source", "git+https://example.invalid/lib"),
            ("req", "^0.1"),
        ] {
            let mut changed = metadata.clone();
            changed["packages"][0]["dependencies"][0][field] = json!(value);
            assert!(validate(lock.as_bytes(), b"synthetic cargo lock", &changed).is_err());
        }
    }

    #[test]
    fn duplicate_unknown_missing_or_unsafe_lock_fields_fail() {
        let (lock, metadata) = fixture();
        for changed in [
            format!("{lock}revision = \"{}\"\n", "a".repeat(40)),
            format!("{lock}extra = \"value\"\n"),
            lock.lines().skip(1).collect::<Vec<_>>().join("\n"),
            lock.replace("lockfile = \"Cargo.lock\"", "lockfile = \"../Cargo.lock\""),
        ] {
            assert!(validate(changed.as_bytes(), b"synthetic cargo lock", &metadata).is_err());
        }
    }
}
