use std::path::Path;
use std::process::Command;

use anyhow::Context;
use base64::prelude::*;
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Provider {
    Github,
    Gitlab,
}

impl Provider {
    fn auth_user(self) -> &'static str {
        match self {
            Provider::Github => "x-access-token",
            // works for personal, group and project access tokens and oauth tokens.
            // TODO: gitlab ci job tokens ($CI_JOB_TOKEN) need the user "gitlab-ci-token" instead
            Provider::Gitlab => "oauth2",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InRepository {
    pub(crate) repository_path: String,
    pub(crate) source_branch: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct OutRepository {
    pub(crate) repository_path: String,
    pub(crate) source_branch: Option<String>,
    // TODO add target_branch
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GitConfig {
    pub(crate) provider: Provider,
    pub(crate) base_url: String,
    #[serde(default)]
    pub(crate) token: String,
    pub(crate) in_repos: Vec<InRepository>,
    pub(crate) out_repo: OutRepository,
}

fn basic_auth_header(user: &str, git_token: &str) -> String {
    let basic_auth_value_in_base64 = BASE64_STANDARD.encode(format!("{user}:{git_token}"));
    format!("Authorization: Basic {basic_auth_value_in_base64}")
}

fn auth_envs(
    provider: Provider,
    base_url: &str,
    git_token: &str,
) -> anyhow::Result<Vec<(&'static str, String)>> {
    let url = parse_base_url(base_url)?;
    anyhow::ensure!(
        url.scheme() == "https",
        "base_url {base_url:?} must use https, the token is never sent over {:?}",
        url.scheme()
    );
    Ok(vec![
        ("GIT_CONFIG_COUNT", "2".to_string()),
        (
            "GIT_CONFIG_KEY_0",
            format!("http.{}/.extraHeader", url.origin().ascii_serialization()),
        ),
        (
            "GIT_CONFIG_VALUE_0",
            basic_auth_header(provider.auth_user(), git_token),
        ),
        ("GIT_CONFIG_KEY_1", "http.followRedirects".to_string()),
        ("GIT_CONFIG_VALUE_1", "false".to_string()),
    ])
}

fn parse_base_url(base_url: &str) -> anyhow::Result<reqwest::Url> {
    let mut base = base_url.to_string();
    if !base.ends_with('/') {
        base.push('/');
    }
    reqwest::Url::parse(&base).with_context(|| format!("invalid base_url {base_url:?}"))
}

fn is_valid_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

fn build_repo_url(base_url: &str, repository_path: &str) -> anyhow::Result<String> {
    anyhow::ensure!(
        repository_path.split('/').all(is_valid_path_segment),
        "invalid repository_path {repository_path:?}: expected a path like group/sub/repo \
         consisting of ascii letters, digits, '-', '_' and '.'"
    );
    let base = parse_base_url(base_url)?;
    let repo_url = base.join(&format!("{repository_path}.git"))?;
    anyhow::ensure!(
        repo_url.origin() == base.origin() && repo_url.path().starts_with(base.path()),
        "repository url {repo_url} is not below base_url {base}"
    );
    Ok(repo_url.into())
}

fn build_clone_args(repo_url: String, source_branch: Option<&str>) -> Vec<String> {
    let mut args = vec!["clone".to_string()];
    if let Some(branch) = source_branch {
        args.push(format!("--branch={branch}"));
    }
    args.push("--".to_string());
    args.push(repo_url);
    args
}

fn run_git(working_dir: &Path, args: &[String], envs: Vec<(&str, String)>) -> anyhow::Result<()> {
    let output = Command::new("git")
        .current_dir(working_dir)
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .envs(envs)
        .args(args)
        .output()
        .with_context(|| {
            format!(
                "running git {} in {}",
                args.join(" "),
                working_dir.display()
            )
        })?;

    if !output.status.success() {
        let err_str = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!(
            "git {} failed ({}): {}{}",
            args.join(" "),
            output.status,
            err_str.trim(),
            redirect_hint(&err_str)
        );
    }

    Ok(())
}

/// redirects are not followed (http.followRedirects=false) so the token is never sent to another
/// url. that also means a moved or renamed repository fails with a bare http 3xx, so explain it
fn redirect_hint(git_stderr: &str) -> &'static str {
    // stable wording, because git runs with LC_ALL=C
    if git_stderr.contains("The requested URL returned error: 3") {
        "\nhint: the server answered with a redirect, which is not followed so that the token is \
         never sent elsewhere. the repository may have moved or been renamed, update \
         repository_path or base_url"
    } else {
        ""
    }
}

pub(crate) fn clone(
    config: &GitConfig,
    working_dir: impl AsRef<Path>,
    repository_path: &str,
    source_branch: Option<&str>,
) -> anyhow::Result<()> {
    let repo_url = build_repo_url(&config.base_url, repository_path)?;
    let auth_env = auth_envs(config.provider, &config.base_url, &config.token)?;
    let clone_args = build_clone_args(repo_url, source_branch);
    run_git(working_dir.as_ref(), &clone_args, auth_env)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPO_URL: &str = "https://gitlab.example.com/group/repo.git";

    /// keeps the developer's global and system git config (commit.gpgsign, http.extraHeader,
    /// url.*.insteadOf, http.proxy, ...) and proxy env vars for the local test servers out of the tests
    fn isolated_envs() -> Vec<(&'static str, String)> {
        vec![
            ("GIT_CONFIG_GLOBAL", "/dev/null".to_string()),
            ("GIT_CONFIG_NOSYSTEM", "1".to_string()),
            ("no_proxy", "127.0.0.1".to_string()),
            ("NO_PROXY", "127.0.0.1".to_string()),
        ]
    }

    /// `auth_envs` plus `isolated_envs`
    fn isolated_auth_envs(provider: Provider, base_url: &str) -> Vec<(&'static str, String)> {
        let mut envs =
            auth_envs(provider, base_url, "test").expect("https base_url to be accepted");
        envs.extend(isolated_envs());
        envs
    }

    fn git_ok(working_dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(working_dir)
            .envs(isolated_envs())
            .args(args)
            .output()
            .expect("to run git");
        assert!(output.status.success(), "git {args:?} failed: {output:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    /// bare repo at `<dir>/source.git` with one commit on `main` and one on `develop`
    fn bare_repo_with_branches(dir: &Path) -> std::path::PathBuf {
        git_ok(dir, &["init", "--bare", "source.git"]);
        git_ok(dir, &["clone", "source.git", "seed"]);
        let seed = dir.join("seed");
        let commit = |msg: &str| {
            git_ok(
                &seed,
                &[
                    "-c",
                    "user.name=t",
                    "-c",
                    "user.email=t@t",
                    "commit",
                    "--allow-empty",
                    "-m",
                    msg,
                ],
            )
        };
        commit("on main");
        git_ok(&seed, &["push", "origin", "HEAD:refs/heads/main"]);
        commit("on develop");
        git_ok(&seed, &["push", "origin", "HEAD:refs/heads/develop"]);
        git_ok(
            dir,
            &[
                "-C",
                "source.git",
                "symbolic-ref",
                "HEAD",
                "refs/heads/main",
            ],
        );
        dir.join("source.git")
    }

    #[test]
    fn provider_deserialize_ok_lowercase_only() {
        #[derive(Deserialize)]
        struct Wrapper {
            provider: Provider,
        }
        let parse = |value: &str| toml::from_str::<Wrapper>(&format!("provider = {value:?}"));

        assert_eq!(parse("github").expect("github").provider, Provider::Github);
        assert_eq!(parse("gitlab").expect("gitlab").provider, Provider::Gitlab);
        for invalid in ["GitHub", "Gitlab", "bitbucket", ""] {
            assert!(parse(invalid).is_err(), "{invalid:?} must be rejected");
        }
    }

    #[test]
    fn git_config_deserialize_ok() {
        let config: GitConfig = toml::from_str(
            r#"
            provider = "gitlab"
            base_url = "https://gitlab.example.com"
            in_repos = [
                { repository_path = "group/service" },
                { repository_path = "group/web", source_branch = "develop" },
            ]
            out_repo = { repository_path = "group/docs", source_branch = "main" }
            "#,
        )
        .expect("config to deserialize");

        assert_eq!(config.provider, Provider::Gitlab);
        assert_eq!(config.base_url, "https://gitlab.example.com");
        assert_eq!(config.token, "", "token defaults to empty");
        let in_repos: Vec<_> = config
            .in_repos
            .iter()
            .map(|r| (r.repository_path.as_str(), r.source_branch.as_deref()))
            .collect();
        assert_eq!(
            in_repos,
            [("group/service", None), ("group/web", Some("develop"))]
        );
        assert_eq!(config.out_repo.repository_path, "group/docs");
        assert_eq!(config.out_repo.source_branch.as_deref(), Some("main"));
    }

    #[test]
    fn git_config_deserialize_err_when_invalid() {
        let valid_out_repo = r#"out_repo = { repository_path = "group/docs" }"#;
        for (case, config) in [
            (
                "missing provider",
                format!("base_url = \"https://x\"\nin_repos = []\n{valid_out_repo}"),
            ),
            (
                "missing base_url",
                format!("provider = \"gitlab\"\nin_repos = []\n{valid_out_repo}"),
            ),
            (
                "unknown field in out_repo",
                "provider = \"gitlab\"\nbase_url = \"https://x\"\nin_repos = []\n\
                 out_repo = { repository_path = \"g/d\", target_branch_prefix = \"doc/\" }"
                    .to_string(),
            ),
            (
                "unknown field in in_repos",
                format!(
                    "provider = \"gitlab\"\nbase_url = \"https://x\"\n\
                     in_repos = [{{ repository_path = \"g/r\", branch = \"main\" }}]\n{valid_out_repo}"
                ),
            ),
            (
                "old string format for in_repos",
                format!(
                    "provider = \"gitlab\"\nbase_url = \"https://x\"\n\
                     in_repos = [\"https://x/g/r.git\"]\n{valid_out_repo}"
                ),
            ),
        ] {
            assert!(
                toml::from_str::<GitConfig>(&config).is_err(),
                "{case} must be rejected"
            );
        }
    }

    #[test]
    fn basic_auth_header_ok() {
        // expected values generated with: echo -n "<user>:test" | base64
        assert_eq!(
            basic_auth_header("x-access-token", "test"),
            "Authorization: Basic eC1hY2Nlc3MtdG9rZW46dGVzdA=="
        );
        assert_eq!(
            basic_auth_header("oauth2", "test"),
            "Authorization: Basic b2F1dGgyOnRlc3Q="
        );
    }

    #[test]
    fn auth_envs_ok_scoped_to_origin() {
        for (provider, base_url, expected_key, expected_user) in [
            (
                Provider::Github,
                "https://github.com",
                "http.https://github.com/.extraHeader",
                "x-access-token",
            ),
            (
                Provider::Gitlab,
                "https://gitlab.example.com/",
                "http.https://gitlab.example.com/.extraHeader",
                "oauth2",
            ),
            (
                Provider::Gitlab,
                "https://GitLab.example.com:8443/gitlab",
                "http.https://gitlab.example.com:8443/.extraHeader",
                "oauth2",
            ),
        ] {
            assert_eq!(
                auth_envs(provider, base_url, "test").expect("https base_url to be accepted"),
                [
                    ("GIT_CONFIG_COUNT", "2".to_string()),
                    ("GIT_CONFIG_KEY_0", expected_key.to_string()),
                    (
                        "GIT_CONFIG_VALUE_0",
                        basic_auth_header(expected_user, "test")
                    ),
                    ("GIT_CONFIG_KEY_1", "http.followRedirects".to_string()),
                    ("GIT_CONFIG_VALUE_1", "false".to_string()),
                ],
                "{base_url}"
            );
        }
    }

    #[test]
    fn auth_envs_err_when_not_https() {
        for base_url in [
            "http://gitlab.example.com",
            "ssh://git@gitlab.example.com",
            "file:///tmp/repos",
            "git@gitlab.example.com:group",
            "gitlab.example.com",
            "",
        ] {
            assert!(
                auth_envs(Provider::Gitlab, base_url, "test").is_err(),
                "{base_url:?} must be rejected"
            );
        }
    }

    #[test]
    fn auth_envs_header_is_applied_by_git_only_for_base_url_host() {
        let dir = tempfile::tempdir().expect("to create a temp dir");
        let envs = isolated_auth_envs(Provider::Gitlab, "https://gitlab.example.com");
        // ask git itself which extraHeader it would send for a given url
        let header_for = |url: &str| {
            let output = Command::new("git")
                .current_dir(dir.path())
                .envs(envs.clone())
                .args(["config", "--get-urlmatch", "http.extraHeader", url])
                .output()
                .expect("to run git config");
            String::from_utf8_lossy(&output.stdout).trim().to_string()
        };

        let expected = basic_auth_header("oauth2", "test");
        assert_eq!(header_for(REPO_URL), expected);
        assert_eq!(
            header_for("https://gitlab.example.com/other/sub/repo.git"),
            expected
        );
        for other in [
            "https://gitlab.example.com.evil.net/group/repo.git",
            "https://gitlab.example.com:8443/group/repo.git",
            "http://gitlab.example.com/group/repo.git",
            "https://github.com/group/repo.git",
        ] {
            assert_eq!(header_for(other), "", "no header for {other}");
        }
    }

    /// answers every connection with `response` and flags that it was contacted
    fn spawn_http_responder(
        response: impl Fn() -> String + Send + 'static,
    ) -> (
        std::net::SocketAddr,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) {
        use std::io::{Read, Write};
        use std::sync::atomic::{AtomicBool, Ordering};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("to bind a local port");
        let addr = listener.local_addr().expect("listener to have an address");
        let contacted = std::sync::Arc::new(AtomicBool::new(false));
        let flag = contacted.clone();
        // detached: the thread ends with the test process
        std::thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                flag.store(true, Ordering::SeqCst);
                let _ = stream.read(&mut [0; 8192]);
                let _ = stream.write_all(response().as_bytes());
            }
        });
        (addr, contacted)
    }

    #[test]
    fn run_git_with_auth_envs_does_not_follow_redirects() {
        let (other_host, other_host_contacted) = spawn_http_responder(|| {
            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string()
        });
        let (redirector, _) = spawn_http_responder(move || {
            format!(
                "HTTP/1.1 301 Moved Permanently\r\n\
                 Location: http://{other_host}/repo.git/info/refs?service=git-upload-pack\r\n\
                 Content-Length: 0\r\nConnection: close\r\n\r\n"
            )
        });
        let target = tempfile::tempdir().expect("to create a temp dir for the clone");
        let envs = isolated_auth_envs(Provider::Gitlab, "https://gitlab.example.com");
        let args = build_clone_args(format!("http://{redirector}/repo.git"), None);

        let err = run_git(target.path(), &args, envs).expect_err("redirect to be refused");

        assert!(err.to_string().contains("301"), "{err}");
        assert!(
            err.to_string()
                .contains("hint: the server answered with a redirect"),
            "{err}"
        );
        assert!(
            !other_host_contacted.load(std::sync::atomic::Ordering::SeqCst),
            "git must not follow the redirect to another host"
        );
    }

    #[test]
    fn redirect_hint_only_for_redirects() {
        for status in [301, 302, 307, 308] {
            let stderr = format!(
                "fatal: unable to access 'https://x/r.git/': The requested URL returned error: {status}"
            );
            assert!(
                redirect_hint(&stderr).contains("repository may have moved"),
                "{status}"
            );
        }
        for stderr in [
            "fatal: unable to access 'https://x/r.git/': The requested URL returned error: 404",
            "fatal: unable to access 'https://x/r.git/': The requested URL returned error: 403",
            "fatal: repository 'x' does not exist",
            "",
        ] {
            assert_eq!(redirect_hint(stderr), "", "{stderr:?}");
        }
    }

    #[test]
    fn build_repo_url_ok() {
        for (base_url, path, expected) in [
            ("https://gitlab.example.com", "group/repo", REPO_URL),
            ("https://gitlab.example.com/", "group/repo", REPO_URL),
            ("https://GitLab.example.com:443", "group/repo", REPO_URL),
            (
                "https://gitlab.example.com",
                "group/sub/my-repo_1.x",
                "https://gitlab.example.com/group/sub/my-repo_1.x.git",
            ),
            (
                "https://gitlab.example.com:8443",
                "group/repo",
                "https://gitlab.example.com:8443/group/repo.git",
            ),
            (
                "https://example.com/gitlab",
                "group/repo",
                "https://example.com/gitlab/group/repo.git",
            ),
        ] {
            assert_eq!(
                build_repo_url(base_url, path).expect("valid repo url"),
                expected,
                "{base_url} + {path}"
            );
        }
    }

    #[test]
    fn build_repo_url_err_when_path_is_invalid() {
        for path in [
            "",
            "/group/repo",
            "group/repo/",
            "group//repo",
            "//gitlab.example.com.evil.net/x",
            "https://gitlab.example.com.evil.net/x",
            "evil.net:443/x",
            "../other/repo",
            "group/../../repo",
            "./repo",
            "%2e%2e/repo",
            "group/repo?x",
            "group/repo#x",
            "user:pw@host/repo",
            "group/re po",
            "group\\repo",
        ] {
            assert!(
                build_repo_url("https://example.com/gitlab", path).is_err(),
                "{path:?} must be rejected"
            );
        }
    }

    #[test]
    fn build_repo_url_err_when_base_url_is_invalid() {
        assert!(build_repo_url("not a url", "group/repo").is_err());
    }

    #[test]
    fn build_clone_args_ok() {
        for (branch, expected) in [
            (None, vec!["clone", "--", REPO_URL]),
            (
                Some("develop"),
                vec!["clone", "--branch=develop", "--", REPO_URL],
            ),
            // an option-like branch stays inside the single `--branch=` argument
            (
                Some("--upload-pack=touch /tmp/pwned"),
                vec![
                    "clone",
                    "--branch=--upload-pack=touch /tmp/pwned",
                    "--",
                    REPO_URL,
                ],
            ),
        ] {
            assert_eq!(
                build_clone_args(REPO_URL.to_string(), branch),
                expected,
                "{branch:?}"
            );
        }
    }

    #[test]
    fn run_git_ok_clones_branch() {
        let dir = tempfile::tempdir().expect("to create a temp dir");
        let source = bare_repo_with_branches(dir.path());
        let source = source.to_str().expect("temp path to be valid UTF-8");

        for (branch, expected_branch) in [(None, "main"), (Some("develop"), "develop")] {
            let target = tempfile::tempdir().expect("to create a temp dir for the clone");
            let args = build_clone_args(source.to_string(), branch);

            run_git(target.path(), &args, isolated_envs()).expect("clone to succeed");

            let clone_dir = target.path().join("source");
            assert_eq!(
                git_ok(&clone_dir, &["branch", "--show-current"]),
                expected_branch
            );
        }
    }

    #[test]
    fn run_git_err_contains_git_stderr() {
        let dir = tempfile::tempdir().expect("to create a temp dir");
        let source = bare_repo_with_branches(dir.path());
        let source = source.to_str().expect("temp path to be valid UTF-8");

        for (args, expected_in_err) in [
            (
                build_clone_args(format!("{source}-missing"), None),
                "does not exist",
            ),
            (
                build_clone_args(source.to_string(), Some("no-such-branch")),
                "Remote branch no-such-branch not found",
            ),
        ] {
            let target = tempfile::tempdir().expect("to create a temp dir for the clone");
            let err = run_git(target.path(), &args, isolated_envs()).expect_err("clone to fail");
            assert!(
                err.to_string().contains(expected_in_err),
                "{expected_in_err:?} not in {err}"
            );
        }
    }

    #[test]
    fn clone_err_before_running_git_when_config_is_invalid() {
        let target = tempfile::tempdir().expect("to create a temp dir for the clone");
        for (base_url, path) in [
            ("http://gitlab.example.com", "group/repo"),
            ("https://gitlab.example.com", "//evil.net/group/repo"),
        ] {
            let config = GitConfig {
                provider: Provider::Gitlab,
                base_url: base_url.to_string(),
                token: "test".to_string(),
                in_repos: Vec::new(),
                out_repo: OutRepository {
                    repository_path: path.to_string(),
                    source_branch: None,
                },
            };
            let result = clone(&config, target.path(), path, None);
            assert!(result.is_err(), "{base_url} + {path} must be rejected");
        }
        assert_eq!(
            std::fs::read_dir(target.path())
                .expect("to read target")
                .count(),
            0,
            "nothing must be cloned"
        );
    }
}
