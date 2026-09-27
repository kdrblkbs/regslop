use std::path::Path;
use std::process::Command;

use anyhow::Context;
use base64::prelude::*;

fn basic_auth_header(git_token: &str) -> String {
    let basic_auth_value_in_base64 = BASE64_STANDARD.encode(format!("x-access-token:{git_token}"));
    format!("Authorization: Basic {basic_auth_value_in_base64}")
}

pub(crate) fn clone(
    git_token: &str,
    working_dir: impl AsRef<Path>,
    repo: &str,
) -> anyhow::Result<()> {
    let args = ["clone", repo];
    let output = Command::new("git")
        .current_dir(&working_dir)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_CONFIG_COUNT", "1")
        // FIX: make this work for gitlab as well
        .env("GIT_CONFIG_KEY_0", "http.https://github.com/.extraHeader")
        .env("GIT_CONFIG_VALUE_0", basic_auth_header(git_token))
        .args(args)
        .output()
        .with_context(|| {
            format!(
                "running git clone of {repo} in {}",
                working_dir.as_ref().display()
            )
        })?;

    if !output.status.success() {
        let err_str = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed ({}): {}",
            args.join(" "),
            output.status,
            err_str.trim()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_auth_header_ok() {
        // expected value generated with: echo -n "x-access-token:test" | base64
        assert_eq!(
            basic_auth_header("test"),
            "Authorization: Basic eC1hY2Nlc3MtdG9rZW46dGVzdA=="
        );
    }

    #[test]
    fn basic_auth_header_has_no_trailing_newline() {
        let header = basic_auth_header("test");
        let encoded = header
            .strip_prefix("Authorization: Basic ")
            .expect("header to start with the basic auth prefix");

        let decoded = BASE64_STANDARD
            .decode(encoded)
            .expect("header value to be valid base64");

        assert_eq!(decoded, b"x-access-token:test");
    }

    #[test]
    fn clone_ok_from_local_bare_repo() {
        let source = tempfile::tempdir().expect("to create a temp dir for the source repo");
        let source_repo = source.path().join("source.git");
        let init = Command::new("git")
            .args(["init", "--bare"])
            .arg(&source_repo)
            .output()
            .expect("to run git init");
        assert!(init.status.success(), "git init --bare failed: {init:?}");
        let target = tempfile::tempdir().expect("to create a temp dir for the clone");

        let repo = source_repo.to_str().expect("temp path to be valid UTF-8");
        clone("unused-token", target.path(), repo).expect("clone to succeed");

        assert!(target.path().join("source").join(".git").is_dir());
    }
}
