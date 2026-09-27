use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;

#[derive(Debug)]
pub(crate) struct RepoDirs {
    pub(crate) in_dir: PathBuf,
    pub(crate) out_dir: PathBuf,
}

fn ignore_if_not_found_or_err(e: io::Error) -> anyhow::Result<()> {
    if e.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(e.into())
    }
}

pub(crate) fn prepare_repo_folders(
    repos_dir: impl AsRef<Path>,
    repos_in_dir: impl AsRef<Path>,
    repos_out_dir: impl AsRef<Path>,
) -> anyhow::Result<RepoDirs> {
    let base = repos_dir.as_ref();
    remove_dir_all(base)
        .or_else(ignore_if_not_found_or_err)
        .with_context(|| format!("removing {}", base.display()))?;

    let in_dir = base.join(repos_in_dir);
    let out_dir = base.join(repos_out_dir);

    create_dir_all(&in_dir).with_context(|| format!("creating {}", in_dir.display()))?;
    create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    Ok(RepoDirs { in_dir, out_dir })
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    const IN: &str = "foo";
    const OUT: &str = "bar";

    #[test]
    fn prepare_repo_folders_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let repos_dir = tmp.path().join("repos");

        let dirs = prepare_repo_folders(&repos_dir, IN, OUT).expect("repo folders to be prepared");

        assert_eq!(dirs.in_dir, repos_dir.join(IN));
        assert_eq!(dirs.out_dir, repos_dir.join(OUT));
        assert!(dirs.in_dir.is_dir());
        assert!(dirs.out_dir.is_dir());
    }

    #[test]
    fn prepare_repo_folders_with_delete_and_recreate_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let repos_dir = tmp.path().join("repos");
        let repos_in_dir = repos_dir.join(IN);
        let repos_in_dir_file = repos_in_dir.join("in.file");
        create_dir_all(&repos_in_dir).expect("to create repos/in in tmp folder");
        File::create(&repos_in_dir_file).expect("to create repos/in/in.file");
        let repos_out_dir = repos_dir.join(OUT);
        let repos_out_dir_file = repos_out_dir.join("out.file");
        create_dir_all(&repos_out_dir).expect("to create repos/out in tmp folder");
        File::create(&repos_out_dir_file).expect("to create repos/out/out.file");

        prepare_repo_folders(&repos_dir, IN, OUT).expect("repo folders to be prepared");

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

        let result = prepare_repo_folders(repos_file, IN, OUT);

        assert!(result.is_err());
    }
}
