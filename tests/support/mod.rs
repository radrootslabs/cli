#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use assert_cmd::prelude::*;
use serde_json::Value;
use tempfile::TempDir;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

pub fn radroots() -> Command {
    Command::cargo_bin("radroots").expect("binary")
}

pub fn json_from_stdout(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout was not json: {error}; stderr `{}`; stdout `{}`",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

pub struct RadrootsCliSandbox {
    root: TempDir,
}

impl RadrootsCliSandbox {
    pub fn new() -> Self {
        Self {
            root: TempDir::new().expect("tempdir"),
        }
    }

    pub fn root(&self) -> &Path {
        self.root.path()
    }

    pub fn command(&self) -> Command {
        let mut command = radroots();
        self.apply_base_env(&mut command);
        command
    }

    pub fn json_success(&self, args: &[&str]) -> Value {
        let output = self.command().args(args).output().expect("run command");
        assert!(
            output.status.success(),
            "`{args:?}` failed with stderr `{}` and stdout `{}`",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
        );
        json_from_stdout(&output)
    }

    pub fn json_output(&self, args: &[&str]) -> (Output, Value) {
        let output = self.command().args(args).output().expect("run command");
        let value = json_from_stdout(&output);
        (output, value)
    }

    pub fn write_workspace_config(&self, raw: &str) -> PathBuf {
        let path = self.root.path().join("config.toml");
        fs::write(&path, raw).expect("write workspace config");
        path
    }

    pub fn write_app_config(&self, raw: &str) -> PathBuf {
        let path = self.root.path().join("config/apps/cli/config.toml");
        fs::create_dir_all(path.parent().expect("app config parent")).expect("app config dir");
        fs::write(&path, raw).expect("write app config");
        path
    }

    #[cfg(unix)]
    pub fn write_fake_myc(&self, name: &str, body: &str) -> PathBuf {
        let path = self.root.path().join("bin").join(name);
        fs::create_dir_all(path.parent().expect("fake myc parent")).expect("fake myc dir");
        fs::write(&path, format!("#!/bin/sh\nset -eu\n{body}\n")).expect("write fake myc");
        let mut permissions = fs::metadata(&path)
            .expect("fake myc metadata")
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&path, permissions).expect("fake myc executable");
        path
    }

    fn apply_base_env(&self, command: &mut Command) {
        command.env("RADROOTS_CLI_PATHS_PROFILE", "repo_local");
        command.env("RADROOTS_CLI_PATHS_REPO_LOCAL_ROOT", self.root.path());
        command.env("RADROOTS_ACCOUNT_SECRET_BACKEND", "encrypted_file");
        command.env("RADROOTS_ACCOUNT_SECRET_FALLBACK", "none");
    }
}
