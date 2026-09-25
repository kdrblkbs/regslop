use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::io;
use std::path::Path;

use anyhow::Context;

const REPOS_IN: &str = "in";
const REPOS_OUT: &str = "out";

fn ignore_if_not_found_or_err(e: io::Error) -> anyhow::Result<()> {
    if e.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(e.into())
    }
}

pub(crate) fn prepare_repo_folders(repos_dir: impl AsRef<Path>) -> anyhow::Result<()> {
    let base = repos_dir.as_ref();
    match remove_dir_all(base) {
        Ok(()) => {}
        Err(e) => ignore_if_not_found_or_err(e).with_context(|| format!("removing {:?}", base))?,
    }

    let in_path = base.join(REPOS_IN);
    let out_path = base.join(REPOS_OUT);

    create_dir_all(&in_path).with_context(|| format!("creating {:?}", in_path))?;
    create_dir_all(&out_path).with_context(|| format!("creating {:?}", out_path))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    #[test]
    fn prepare_repo_folders_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let repos_dir = tmp.path().join("repos");

        prepare_repo_folders(&repos_dir).expect("repo folders to be prepared");

        assert!(repos_dir.join(REPOS_IN).is_dir());
        assert!(repos_dir.join(REPOS_OUT).is_dir());
    }

    #[test]
    fn prepare_repo_folders_with_delete_and_recreate_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let repos_dir = tmp.path().join("repos");
        let repos_in_dir = repos_dir.join(REPOS_IN);
        let repos_in_dir_file = repos_in_dir.join("in.file");
        create_dir_all(&repos_in_dir).expect("to create repos/in in tmp folder");
        File::create(&repos_in_dir_file).expect("to create repos/in/in.file");
        let repos_out_dir = repos_dir.join(REPOS_OUT);
        let repos_out_dir_file = repos_out_dir.join("out.file");
        create_dir_all(&repos_out_dir).expect("to create repos/out in tmp folder");
        File::create(&repos_out_dir_file).expect("to create repos/out/out.file");

        prepare_repo_folders(&repos_dir).expect("repo folders to be prepared");

        assert!(repos_in_dir.is_dir());
        assert!(repos_out_dir.is_dir());
        assert!(!repos_in_dir_file.exists());
        assert!(!repos_out_dir_file.exists());
    }

    #[test]
    fn prepare_repo_folders_err_when_path_is_file() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let repos_file = tmp.path().join("repos");
        File::create(&repos_file).expect("to create file");

        let result = prepare_repo_folders(repos_file);

        assert!(result.is_err());
    }
}
