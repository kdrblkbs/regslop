use std::fs::create_dir_all;
use std::fs::remove_dir_all;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;

#[derive(Debug)]
pub(crate) struct Sandbox {
    pub(crate) root: PathBuf,
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

pub(crate) fn prepare_sandbox(
    sandbox_root: impl AsRef<Path>,
    sandbox_in: impl AsRef<Path>,
    sandbox_out: impl AsRef<Path>,
) -> anyhow::Result<Sandbox> {
    let base = sandbox_root.as_ref();
    remove_dir_all(base)
        .or_else(ignore_if_not_found_or_err)
        .with_context(|| format!("removing {}", base.display()))?;

    create_dir_all(base).with_context(|| format!("creating {}", base.display()))?;
    // canonical, because the tools check every path with starts_with against the root. a relative
    // or symlinked root would deny every access. canonicalize needs the dir to exist
    let root = base
        .canonicalize()
        .with_context(|| format!("canonicalizing {}", base.display()))?;

    let in_dir = root.join(sandbox_in);
    let out_dir = root.join(sandbox_out);

    create_dir_all(&in_dir).with_context(|| format!("creating {}", in_dir.display()))?;
    create_dir_all(&out_dir).with_context(|| format!("creating {}", out_dir.display()))?;

    Ok(Sandbox {
        root,
        in_dir,
        out_dir,
    })
}

#[cfg(test)]
mod tests {
    use std::fs::File;

    use super::*;

    const IN: &str = "foo";
    const OUT: &str = "bar";

    #[test]
    fn prepare_sandbox_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let sandbox_root = tmp.path().join("sandbox");

        let sandbox = prepare_sandbox(&sandbox_root, IN, OUT).expect("sandbox to be prepared");

        // the temp dir itself may be below a symlink (e.g. /var -> /private/var on macOS)
        let expected_root = sandbox_root
            .canonicalize()
            .expect("to canonicalize the sandbox root");
        assert_eq!(sandbox.root, expected_root);
        assert_eq!(sandbox.in_dir, expected_root.join(IN));
        assert_eq!(sandbox.out_dir, expected_root.join(OUT));
        assert!(sandbox.in_dir.is_dir());
        assert!(sandbox.out_dir.is_dir());
    }

    #[test]
    fn prepare_sandbox_ok_root_is_canonical() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let tmp_path = tmp
            .path()
            .canonicalize()
            .expect("to canonicalize the temp dir");
        create_dir_all(tmp_path.join("real")).expect("to create real");
        std::os::unix::fs::symlink(tmp_path.join("real"), tmp_path.join("link"))
            .expect("to create the symlink");

        for (sandbox_root, expected_root) in [
            (tmp_path.join("sub/../sandbox"), tmp_path.join("sandbox")),
            (tmp_path.join("link/sandbox"), tmp_path.join("real/sandbox")),
        ] {
            let sandbox = prepare_sandbox(&sandbox_root, IN, OUT).expect("sandbox to be prepared");

            assert_eq!(sandbox.root, expected_root, "{}", sandbox_root.display());
            assert_eq!(sandbox.in_dir, expected_root.join(IN));
            assert_eq!(sandbox.out_dir, expected_root.join(OUT));
        }
    }

    #[test]
    fn prepare_sandbox_with_delete_and_recreate_ok() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let sandbox_root = tmp.path().join("sandbox");
        let sandbox_in_dir = sandbox_root.join(IN);
        let sandbox_in_dir_file = sandbox_in_dir.join("in.file");
        create_dir_all(&sandbox_in_dir).expect("to create sandbox/in in tmp folder");
        File::create(&sandbox_in_dir_file).expect("to create sandbox/in/in.file");
        let sandbox_out_dir = sandbox_root.join(OUT);
        let sandbox_out_dir_file = sandbox_out_dir.join("out.file");
        create_dir_all(&sandbox_out_dir).expect("to create sandbox/out in tmp folder");
        File::create(&sandbox_out_dir_file).expect("to create sandbox/out/out.file");
        let sandbox_root_file = sandbox_root.join("root.file");
        File::create(&sandbox_root_file).expect("to create sandbox/root.file");

        prepare_sandbox(&sandbox_root, IN, OUT).expect("sandbox to be prepared");

        assert!(sandbox_in_dir.is_dir());
        assert!(sandbox_out_dir.is_dir());
        assert!(!sandbox_in_dir_file.exists());
        assert!(!sandbox_out_dir_file.exists());
        assert!(!sandbox_root_file.exists());
    }

    #[test]
    fn prepare_sandbox_err_when_path_is_file() {
        let tmp = tempfile::tempdir().expect("to create a temp dir");
        let sandbox_file = tmp.path().join("sandbox");
        File::create(&sandbox_file).expect("to create file");

        let result = prepare_sandbox(sandbox_file, IN, OUT);

        assert!(result.is_err());
    }
}
