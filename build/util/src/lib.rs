// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, Context, Result};
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;

/// Reads the given environment variable and marks that it's used
///
/// This ensures a rebuild if the variable changes
pub fn env_var(key: &str) -> Result<String, std::env::VarError> {
    println!("cargo:rerun-if-env-changed={}", key);
    std::env::var(key)
}

/// Reads the `OUT_DIR` environment variable
///
/// This function goes through `std::env::var` directly, rather than our own
/// `env_var`, because Cargo should know when it changes.
pub fn out_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("OUT_DIR").expect("Could not get OUT_DIR"),
    )
}

/// Reads the `TARGET` environment variable
///
/// This function goes through `std::env::var` directly, rather than our own
/// `env_var`, because Cargo should know when `OUT_DIR` changes.
pub fn target() -> String {
    std::env::var("TARGET").unwrap()
}

/// Reads the `TARGET_OS` environment variable
///
/// This function goes through `std::env::var` directly, rather than our own
/// `env_var`, because Cargo should know when it changes.
pub fn target_os() -> String {
    std::env::var("CARGO_CFG_TARGET_OS").unwrap()
}

/// Checks to see whether the given feature is enabled
pub fn has_feature(s: &str) -> bool {
    std::env::var(format!(
        "CARGO_FEATURE_{}",
        s.to_uppercase().replace("-", "_")
    ))
    .is_ok()
}

/// Exposes the CPU's M-profile architecture version. This isn't available in
/// rustc's standard environment.
///
/// This will set one of `cfg(armv6m`), `cfg(armv7m)`, or `cfg(armv8m)`
/// depending on the value of the `TARGET` environment variable.
pub fn expose_m_profile() {
    let target = crate::target();

    if target.starts_with("thumbv6m") {
        println!("cargo:rustc-cfg=armv6m");
    } else if target.starts_with("thumbv7m") || target.starts_with("thumbv7em")
    {
        println!("cargo:rustc-cfg=armv7m");
    } else if target.starts_with("thumbv8m") {
        println!("cargo:rustc-cfg=armv8m");
    } else {
        println!("Don't know the target {}", target);
        std::process::exit(1);
    }
}

/// Exposes the board type from the `HUBRIS_BOARD` envvar into
/// `cfg(target_board="...")`.
pub fn expose_target_board() {
    if let Ok(board) = crate::env_var("HUBRIS_BOARD") {
        println!("cargo:rustc-cfg=target_board=\"{}\"", board);
    }
}

///
/// Pulls the app-wide configuration for purposes of a build task.  This
/// will fail if the app-wide configuration doesn't exist or can't parse.
/// Note that -- thanks to the magic of Serde -- `T` need not (and indeed,
/// should not) contain the entire app-wide configuration, but rather only
/// those parts that a particular build task cares about.  (It should go
/// without saying that `deny_unknown_fields` should *not* be set on this
/// type -- but it may well be set within the task-specific types that
/// this type contains.)  If the configuration field is optional, `T` should
/// reflect that by having its member (or members) be an `Option` type.
///
pub fn config<T: DeserializeOwned>() -> Result<T> {
    toml_from_env("HUBRIS_APP_CONFIG")?.ok_or_else(|| {
        anyhow!("app.toml missing global config section [config]")
    })
}

/// Pulls the task configuration. See `config` for more details.
pub fn task_config<T: DeserializeOwned>() -> Result<T> {
    let task_name =
        crate::env_var("HUBRIS_TASK_NAME").expect("missing HUBRIS_TASK_NAME");
    task_maybe_config()?.ok_or_else(|| {
        anyhow!(
            "app.toml missing task config section [tasks.{}.config]",
            task_name
        )
    })
}

/// Pulls the task configuration, or `None` if the configuration is not
/// provided.
pub fn task_maybe_config<T: DeserializeOwned>() -> Result<Option<T>> {
    toml_from_env("HUBRIS_TASK_CONFIG")
}

/// Returns a map of task names to their IDs.
pub fn task_ids() -> TaskIds {
    let tasks = crate::env_var("HUBRIS_TASKS").expect("missing HUBRIS_TASKS");
    TaskIds(
        tasks
            .split(',')
            .enumerate()
            .map(|(i, name)| (name.to_string(), i))
            .collect(),
    )
}

/// Map of task names to their IDs.
pub struct TaskIds(BTreeMap<String, usize>);

impl TaskIds {
    /// Get the ID of a task by name.
    pub fn get(&self, task_name: &str) -> Option<usize> {
        self.0.get(task_name).copied()
    }

    /// Convert a list of task names into a list of task IDs, ordered the same.
    pub fn names_to_ids<S>(&self, names: &[S]) -> Result<Vec<usize>>
    where
        S: AsRef<str>,
    {
        names
            .iter()
            .map(|name| {
                let name = name.as_ref();
                self.get(name)
                    .ok_or_else(|| anyhow!("unknown task `{}`", name))
            })
            .collect()
    }

    /// Helper function to convert a map of operation names to allowed callers
    /// (by name) to a map of operation names to allowed callers (by task ID).
    pub fn remap_allowed_caller_names_to_ids(
        &self,
        allowed_callers: &BTreeMap<String, Vec<String>>,
    ) -> Result<BTreeMap<String, Vec<usize>>> {
        allowed_callers
            .iter()
            .map(|(name, tasks)| {
                let task_ids = self.names_to_ids(tasks)?;
                Ok((name.clone(), task_ids))
            })
            .collect()
    }
}

/// Parse the contents of an environment variable as toml.
///
/// Returns:
///
/// - `Ok(Some(x))` if the environment variable is defined and the contents
///   deserialized correctly.
/// - `Ok(None)` if the environment variable is not defined.
/// - `Err(e)` if deserialization failed or the environment variable did not
///   contain UTF-8.
fn toml_from_env<T: DeserializeOwned>(var: &str) -> Result<Option<T>> {
    let config = match crate::env_var(var) {
        Err(std::env::VarError::NotPresent) => return Ok(None),
        Err(e) => {
            return Err(e).with_context(|| {
                format!("accessing environment variable {}", var)
            })
        }
        Ok(c) => c,
    };

    println!("--- toml for ${} ---", var);
    println!("{}", config);
    let rval = toml::from_slice(config.as_bytes())
        .context("deserializing configuration")?;
    Ok(Some(rval))
}
