use std::path::Path;
use std::process::Command;

use anyhow::Context;
use base64::prelude::*;
use reqwest::Url;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    // user of the basic auth credentials, the token is the password
    fn auth_user(self) -> &'static str {
        match self {
            Provider::Github => "x-access-token",
            // gitlab accepts oauth2 as user for personal, project and group access tokens
            Provider::Gitlab => "oauth2",
        }
    }
}

fn basic_auth_header(user: &str, git_token: &str) -> String {
    let basic_auth_value_in_base64 = BASE64_STANDARD.encode(format!("{user}:{git_token}"));
    format!("Authorization: Basic {basic_auth_value_in_base64}")
}

// the header is scoped to the origin of the repo url, so the token is only sent to that host (e.g.
// a self-hosted gitlab). anything that is not https (local paths, ssh, plain http) gets no header,
// the token must never go out unencrypted
fn auth_envs(provider: Provider, git_token: &str, repo: &str) -> Vec<(&'static str, String)> {
    let Ok(url) = Url::parse(repo) else {
        return Vec::new();
    };
    if url.scheme() != "https" {
        return Vec::new();
    }
    vec![
        ("GIT_CONFIG_COUNT", "1".to_string()),
        (
            "GIT_CONFIG_KEY_0",
            format!("http.{}/.extraHeader", url.origin().ascii_serialization()),
        ),
        (
            "GIT_CONFIG_VALUE_0",
            basic_auth_header(provider.auth_user(), git_token),
        ),
    ]
}

pub(crate) fn clone(
    provider: Provider,
    git_token: &str,
    working_dir: impl AsRef<Path>,
    repo: &str,
) -> anyhow::Result<()> {
    let args = ["clone", repo];
    let output = Command::new("git")
        .current_dir(&working_dir)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .envs(auth_envs(provider, git_token, repo))
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
        // expected values generated with: echo -n "x-access-token:test" | base64
        assert_eq!(
            basic_auth_header("x-access-token", "test"),
            "Authorization: Basic eC1hY2Nlc3MtdG9rZW46dGVzdA=="
        );
        // echo -n "oauth2:test" | base64
        assert_eq!(
            basic_auth_header("oauth2", "test"),
            "Authorization: Basic b2F1dGgyOnRlc3Q="
        );
    }

    #[test]
    fn basic_auth_header_has_no_trailing_newline() {
        let header = basic_auth_header("x-access-token", "test");
        let encoded = header
            .strip_prefix("Authorization: Basic ")
            .expect("header to start with the basic auth prefix");

        let decoded = BASE64_STANDARD
            .decode(encoded)
            .expect("header value to be valid base64");

        assert_eq!(decoded, b"x-access-token:test");
    }

    #[test]
    fn auth_envs_ok_scoped_to_origin() {
        for (provider, repo, expected_key, expected_user) in [
            (
                Provider::Github,
                "https://github.com/example-org/repo.git",
                "http.https://github.com/.extraHeader",
                "x-access-token",
            ),
            (
                Provider::Gitlab,
                "https://gitlab.example.com/group/sub/repo.git",
                "http.https://gitlab.example.com/.extraHeader",
                "oauth2",
            ),
            (
                Provider::Gitlab,
                "https://gitlab.example.com:8443/group/repo.git",
                "http.https://gitlab.example.com:8443/.extraHeader",
                "oauth2",
            ),
        ] {
            assert_eq!(
                auth_envs(provider, "test", repo),
                vec![
                    ("GIT_CONFIG_COUNT", "1".to_string()),
                    ("GIT_CONFIG_KEY_0", expected_key.to_string()),
                    (
                        "GIT_CONFIG_VALUE_0",
                        basic_auth_header(expected_user, "test")
                    ),
                ],
                "{repo}"
            );
        }
    }

    #[test]
    fn auth_envs_empty_when_not_https() {
        for repo in [
            "http://gitlab.example.com/group/repo.git",
            "ssh://git@gitlab.example.com/group/repo.git",
            "git@gitlab.example.com:group/repo.git",
            "/tmp/source.git",
        ] {
            assert!(
                auth_envs(Provider::Gitlab, "test", repo).is_empty(),
                "{repo}"
            );
        }
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
        clone(Provider::Gitlab, "unused-token", target.path(), repo).expect("clone to succeed");

        assert!(target.path().join("source").join(".git").is_dir());
    }
}
