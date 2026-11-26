use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use clap::Parser;
use serde::Deserialize;

mod converse;
mod git;
mod harness;
mod tools;
mod util;

const SANDBOX_ROOT: &str = "./regslop_sandbox";
const SANDBOX_IN: &str = "in";
const SANDBOX_OUT: &str = "out";

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Args {
    /// Path to the config file
    #[arg(short, long, default_value("./regslop_config.toml"))]
    config: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BedrockConfig {
    max_iterations: usize,
    client_timeout_mins: u64,
    max_tokens: u32,
    temperature: Option<f32>,
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
    git: git::GitConfig,
    bedrock: BedrockConfig,
}

fn load_config(config_path: &Path) -> anyhow::Result<Config> {
    let config_content = std::fs::read_to_string(config_path)
        .with_context(|| format!("failed to read config file at {}", config_path.display()))?;
    let config: Config = toml::from_str(&config_content)
        .with_context(|| format!("failed to deserialize config at {}", config_path.display()))?;
    Ok(config)
}

fn clone_repos(git_config: &git::GitConfig, sandbox: &util::Sandbox) -> anyhow::Result<()> {
    for repo in &git_config.in_repos {
        git::clone(
            git_config,
            &sandbox.in_dir,
            &repo.repository_path,
            repo.source_branch.as_deref(),
        )?;
    }
    let out_repo = &git_config.out_repo;
    git::clone(
        git_config,
        &sandbox.out_dir,
        &out_repo.repository_path,
        out_repo.source_branch.as_deref(),
    )
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let config = load_config(&args.config)?;
    // TODO: check if env contains git/bedrock token
    // TODO: token from env -> token from file
    // TODO: error if any token == ""
    // there is probably a crate that does all of this
    let sandbox = util::prepare_sandbox(SANDBOX_ROOT, SANDBOX_IN, SANDBOX_OUT)?;
    clone_repos(&config.git, &sandbox)?;

    harness::run(&sandbox.root, &config.bedrock)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_config_deserializes() {
        toml::from_str::<Config>(include_str!("../regslop_config.example.toml"))
            .expect("example config to match the config structs");
    }
}
