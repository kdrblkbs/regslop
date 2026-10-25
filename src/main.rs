use anyhow::Context;
use serde::Deserialize;

mod converse;
mod git;
mod harness;
mod tools;
mod util;

const SANDBOX_ROOT: &str = "./regslop_sandbox";
const SANDBOX_IN: &str = "in";
const SANDBOX_OUT: &str = "out";
// TODO: this is probably configurable soon (TM)
const CONFIG_PATH: &str = "regslop_config.toml";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GitConfig {
    provider: git::Provider,
    #[serde(default)]
    token: String,
    in_repos: Vec<String>,
    out_repo: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BedrockConfig {
    max_iterations: usize,
    client_timeout_mins: u64,
    max_tokens: u32,
    #[serde(default)]
    token: String,
    base_url: String,
    model: String,
    system_prompt: String,
    task_prompt: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    git: GitConfig,
    bedrock: BedrockConfig,
}

fn load_config() -> anyhow::Result<Config> {
    let config_content = std::fs::read_to_string(CONFIG_PATH)
        .with_context(|| format!("failed to read config file at {CONFIG_PATH}"))?;
    let config: Config = toml::from_str(&config_content)
        .with_context(|| format!("failed to deserialize config at {CONFIG_PATH}"))?;
    Ok(config)
}

fn clone_repos(
    provider: git::Provider,
    git_token: &str,
    in_repos: &[String],
    out_repo: &str,
    sandbox: &util::Sandbox,
) -> anyhow::Result<()> {
    for repo in in_repos {
        git::clone(provider, git_token, &sandbox.in_dir, repo)?;
    }
    git::clone(provider, git_token, &sandbox.out_dir, out_repo)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    // TODO: check if env contains git/bedrock token
    // TODO: token from env -> token from file
    // TODO: error if any token == ""
    // there is probably a crate that does all of this
    let sandbox = util::prepare_sandbox(SANDBOX_ROOT, SANDBOX_IN, SANDBOX_OUT)?;
    clone_repos(
        config.git.provider,
        &config.git.token,
        &config.git.in_repos,
        &config.git.out_repo,
        &sandbox,
    )?;

    harness::run(&sandbox.root, &config.bedrock)
}
