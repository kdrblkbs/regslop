mod git;
mod util;

const REPOS_ROOT: &str = "./repos_foo_bar";
const REPOS_IN: &str = "in";
const REPOS_OUT: &str = "out";

fn clone_repos(
    git_token: &str,
    in_repos: &[&str],
    out_repo: &str,
    repo_dirs: &util::RepoDirs,
) -> anyhow::Result<()> {
    for repo in in_repos {
        git::clone(git_token, &repo_dirs.in_dir, repo)?;
    }
    git::clone(git_token, &repo_dirs.out_dir, out_repo)?;
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let git_token = "fake_token";
    let git_token = git_token.trim();
    let in_repos = ["https://github.com/kdrblkbs/calculator-service.git"];
    let out_repo = "https://github.com/kdrblkbs/calculator-service-regulatory.git";

    let repo_dirs = util::prepare_repo_folders(REPOS_ROOT, REPOS_IN, REPOS_OUT)?;
    clone_repos(git_token, &in_repos, out_repo, &repo_dirs)?;

    Ok(())
}
