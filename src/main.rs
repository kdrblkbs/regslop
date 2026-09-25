mod git;
mod util;

const REPOS_ROOT: &str = "./repos";

fn main() -> anyhow::Result<()> {
    util::prepare_repo_folders(REPOS_ROOT)?;

    let repos = [
        "https://github.com/tokio-rs/tokio.git",
        "https://github.com/tokio-rs/mio.git",
        "https://github.com/tokio-rs/bytes.git",
    ];
    for repo in repos {
        println!("Hello, world! {repo}");
        git::clone()?;
    }

    Ok(())
}
