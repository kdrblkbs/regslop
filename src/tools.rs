use std::env;
use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde_json::Value;
use serde_json::json;

// TODO: read and write operate on a single file, rename to read_file / write_file. also the write
// description should state that it overwrites the whole file and that the parent dir must exist.
const TOOL_LIST: &str = "list_files";
const TOOL_READ: &str = "read_files";
const TOOL_WRITE: &str = "write_files";

pub(crate) struct Tool {
    pub description: String,
    pub schema: Value,
}

// TODO: we need more tools.
// * something like grep
// * something to create patches (aka small writes in files)
// * generally improve read and list, refer to todos below
pub(crate) fn get_all() -> Vec<(String, Tool)> {
    vec![
        (
            TOOL_LIST.to_string(),
            Tool {
                description: "List files (recursively) in a directory of the current project."
                    .to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Directory, e.g. '.'"}},
                    "required": ["path"]
                }),
            },
        ),
        (
            TOOL_READ.to_string(),
            Tool {
                description: "Read a text file from the current project.".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "File path, e.g. 'README.md'"}},
                    "required": ["path"]
                }),
            },
        ),
        (
            TOOL_WRITE.to_string(),
            Tool {
                description: "Write a text file into the current project.".to_string(),
                schema: json!({
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "File path, e.g. 'README.md'"},
                        "contents": {"type": "string", "description": "The file content to write"}
                    },
                    "required": ["path", "contents"]
                }),
            },
        ),
    ]
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PathArgs {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WriteArgs {
    path: String,
    contents: String,
}

fn resolve_in_sandbox(path: &Path) -> anyhow::Result<PathBuf> {
    let current_working_directory = env::current_dir().and_then(fs::canonicalize)?;
    let resolved = current_working_directory.join(path).canonicalize()?;
    if !resolved.starts_with(&current_working_directory) {
        anyhow::bail!(
            "access denied: '{}' is outside of the working directory, use a relative path within the project",
            path.display()
        );
    }
    Ok(resolved)
}

// TODO: don't use the current working directory as sandbox root, pass explicit roots instead.
// * read only within repos_foo_bar/in and repos_foo_bar/out
// * write only within repos_foo_bar/out
// * canonicalize the roots once when the sandbox is created, not on every call
// * check writes against the write root only. a shared "inside any root" helper would also
//   accept writes into repos_foo_bar/in
// * the tests serialize on CWD_LOCK because of the cwd root, remove the lock afterwards
// TODO: errors are passed to the model as tool results, make them actionable. e.g. ELOOP when
// writing through a symlink shows up as "Too many levels of symbolic links", and a missing path
// fails in resolve_in_sandbox (canonicalize) without naming the path.
pub(crate) fn run(name: &str, args: Value) -> anyhow::Result<String> {
    match name {
        TOOL_LIST => {
            let args: PathArgs = serde_json::from_value(args)?;
            let path = resolve_in_sandbox(Path::new(&args.path))?;
            list_recursive(&path)
        }
        TOOL_READ => {
            let args: PathArgs = serde_json::from_value(args)?;
            let path = resolve_in_sandbox(Path::new(&args.path))?;
            read_file(&path)
        }
        TOOL_WRITE => {
            let args: WriteArgs = serde_json::from_value(args)?;
            let path_from_args = Path::new(&args.path);
            // checked on the raw string, because Path ignores a trailing "/" or "/." and "foo/"
            // would otherwise write the file "foo" (or overwrite it, if it exists)
            let ends_like_a_dir = args.path.ends_with('/') || args.path.ends_with("/.");
            let (Some(parent), Some(file_name)) = (
                path_from_args.parent(),
                path_from_args.file_name().filter(|_| !ends_like_a_dir),
            ) else {
                anyhow::bail!(
                    "invalid path '{}': expected a path to a file, e.g. 'docs/summary.md'",
                    args.path
                );
            };
            // the parent is checked instead of the full path, because the file may not exist yet
            let parent_dir = resolve_in_sandbox(parent)
                .map_err(|e| anyhow::anyhow!("cannot write '{}': {e}", args.path))?;
            write_file(&parent_dir.join(file_name), &args.contents)
        }
        _ => anyhow::bail!("no tool for {name} with args {args}"),
    }
}

// TODO: add offset and number of lines to read
// map_err instead of with_context, because the harness only passes the top level error message
// to the model and the cause would be lost otherwise
fn read_file(path: &Path) -> anyhow::Result<String> {
    fs::read_to_string(path).map_err(|e| anyhow::anyhow!("cannot read '{}': {e}", path.display()))
}

fn write_file(path: &Path, contents: &str) -> anyhow::Result<String> {
    let with_path = |e| anyhow::anyhow!("cannot write '{}': {e}", path.display());
    let mut file = File::options()
        .create(true)
        .write(true)
        .truncate(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(with_path)?;
    file.write_all(contents.as_bytes()).map_err(with_path)?;
    // TODO: include path and byte count so the model gets real confirmation
    Ok("successfully wrote file".to_string())
}

// TODO: add depth for recursive
// TODO: respect .gitignore
// TODO: compact result if too big
// TODO: remove working dir prefix
// TODO: an empty directory returns "" as success, return an explicit message instead
fn list_recursive(path: &Path) -> anyhow::Result<String> {
    let mut files = Vec::new();
    read_dir_recursive(path, &mut files)?;
    files.sort();
    Ok(files
        .iter()
        .map(|f| f.display().to_string())
        .collect::<Vec<String>>()
        .join("\n"))
}

// TODO: this skip list is cosmetic, not access control. read and write must also
// refuse any path with a .git component, otherwise e.g. writing .git/config of a
// cloned repo allows command execution the next time we run git on it.
// TODO: names are matched at any depth, so e.g. a legit docs/target/ is hidden too.
const SKIPPED_NAMES: &[&str] = &[".git", "target", ".env"];

// TODO: a single unreadable entry (e.g. permission denied, file deleted during the walk) aborts the
// whole listing. skip or annotate such entries instead.
fn read_dir_recursive(dir: &Path, out: &mut Vec<PathBuf>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if SKIPPED_NAMES.iter().any(|skipped| name == *skipped) {
            continue;
        }
        // file_type() doesn't follow symlinks, so symlinked dirs won't cause loops
        if entry.file_type()?.is_dir() {
            read_dir_recursive(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    use std::sync::Mutex;
    use std::sync::MutexGuard;
    use std::sync::PoisonError;

    use tempfile::TempDir;

    use super::*;

    // run() resolves paths against the process wide current directory and cargo runs tests in
    // parallel threads, so every test that changes it must hold this lock.
    static CWD_LOCK: Mutex<()> = Mutex::new(());

    // creates <tmp>/root as sandbox (and current directory) and <tmp>/outside/secret next to it,
    // so escape attempts hit a real file and "access denied" can't be confused with "not found".
    // restores the previous current directory on drop. fields are dropped in declaration order:
    // the temp dir is deleted before the lock is released.
    struct TestDirs {
        dir: TempDir,
        previous: PathBuf,
        _lock: MutexGuard<'static, ()>,
    }

    impl TestDirs {
        fn new() -> Self {
            // a failed test poisons the lock, which must not fail all the following tests
            let lock = CWD_LOCK.lock().unwrap_or_else(PoisonError::into_inner);
            let previous = env::current_dir().expect("current dir to be readable");
            let dir = tempfile::tempdir().expect("to create a temp dir");
            fs::create_dir(dir.path().join("root")).expect("to create root");
            fs::create_dir(dir.path().join("outside")).expect("to create outside");
            fs::write(dir.path().join("outside/secret"), "secret").expect("to create the secret");
            env::set_current_dir(dir.path().join("root")).expect("to change into root");
            Self {
                dir,
                previous,
                _lock: lock,
            }
        }

        fn root(&self) -> PathBuf {
            self.dir.path().join("root")
        }

        fn join(&self, path: impl AsRef<Path>) -> PathBuf {
            self.root().join(path)
        }

        fn outside(&self, path: &str) -> PathBuf {
            self.dir.path().join("outside").join(path)
        }

        // creates a file below root, including missing parent dirs
        fn create(&self, path: impl AsRef<Path>, contents: &str) {
            let path = self.join(path);
            let parent = path.parent().expect("file below root to have a parent");
            fs::create_dir_all(parent).expect("to create the parent dirs");
            fs::write(&path, contents).expect("to create the file");
        }

        fn contents(&self, path: &str) -> String {
            fs::read_to_string(self.join(path)).expect("to read the file")
        }

        fn secret(&self) -> String {
            fs::read_to_string(self.outside("secret")).expect("to read the secret")
        }
    }

    impl Drop for TestDirs {
        fn drop(&mut self) {
            // drop also runs while a failed test unwinds, panicking here would abort the test binary
            let _ = env::set_current_dir(&self.previous);
        }
    }

    fn list(path: &str) -> anyhow::Result<String> {
        run(TOOL_LIST, json!({"path": path}))
    }

    fn read(path: &str) -> anyhow::Result<String> {
        run(TOOL_READ, json!({"path": path}))
    }

    fn write(path: &str, contents: &str) -> anyhow::Result<String> {
        run(TOOL_WRITE, json!({"path": path, "contents": contents}))
    }

    fn path_str(path: &Path) -> &str {
        path.to_str().expect("temp path to be valid UTF-8")
    }

    // `call` names the failing call in the panic message, which matters for table driven tests
    fn assert_err_contains(result: anyhow::Result<String>, expected: &str, call: &str) {
        let err = result.expect_err(&format!("{call} to fail"));
        assert!(
            err.to_string().contains(expected),
            "expected '{expected}' in the error of {call}, got: {err}"
        );
    }

    // the listing contains absolute paths (TODO: remove working dir prefix), strip the root so
    // the assertions don't depend on the temp dir location
    fn listed(tmp: &TestDirs, path: &str) -> Vec<String> {
        let root = fs::canonicalize(tmp.root()).expect("to canonicalize root");
        let prefix = format!("{}/", root.display());
        list(path)
            .expect("listing to succeed")
            .lines()
            .map(|line| {
                line.strip_prefix(&prefix)
                    .unwrap_or_else(|| panic!("'{line}' to start with the root"))
                    .to_string()
            })
            .collect()
    }

    // get_all and run

    #[test]
    fn get_all_returns_tools_with_object_schemas() {
        let tools = get_all();

        let names: Vec<&str> = tools.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, [TOOL_LIST, TOOL_READ, TOOL_WRITE]);
        for (name, tool) in &tools {
            assert!(!tool.description.is_empty(), "{name} has no description");
            assert_eq!(tool.schema["type"], "object", "{name}");
            let required = tool.schema["required"]
                .as_array()
                .expect("required to be an array");
            for field in required {
                let field = field.as_str().expect("required field to be a string");
                assert!(
                    tool.schema["properties"][field].is_object(),
                    "{name}: '{field}' is required but has no property"
                );
            }
        }
    }

    // guards against the json schema and the args structs drifting apart
    #[test]
    fn run_accepts_exactly_the_required_fields_of_each_schema() {
        let _tmp = TestDirs::new();

        for (name, tool) in get_all() {
            let args: serde_json::Map<String, Value> = tool.schema["required"]
                .as_array()
                .expect("required to be an array")
                .iter()
                .map(|field| {
                    let field = field.as_str().expect("required field to be a string");
                    (field.to_string(), json!("x"))
                })
                .collect();

            // the path "x" doesn't exist for list and read, only argument errors matter here
            if let Err(err) = run(&name, Value::Object(args)) {
                let err = err.to_string();
                for unexpected in ["missing field", "unknown field", "no tool for"] {
                    assert!(!err.contains(unexpected), "{name}: {err}");
                }
            }
        }
    }

    #[test]
    fn run_err_when_tool_is_unknown() {
        let _tmp = TestDirs::new();

        assert_err_contains(
            run("delete_files", json!({"path": "x"})),
            "no tool for delete_files",
            "delete_files",
        );
    }

    #[test]
    fn run_err_when_args_do_not_match_schema() {
        let tmp = TestDirs::new();
        tmp.create("a", "original");

        let cases = [
            (TOOL_LIST, json!({}), "missing field `path`"),
            (TOOL_LIST, json!("a"), "invalid type"),
            (TOOL_READ, json!({"path": 1}), "invalid type"),
            (
                TOOL_READ,
                json!({"path": "a", "offset": 1}),
                "unknown field `offset`",
            ),
            (TOOL_WRITE, json!({"path": "a"}), "missing field `contents`"),
            (
                TOOL_WRITE,
                json!({"path": "a", "content": "overwritten"}),
                "unknown field `content`",
            ),
        ];
        for (name, args, expected) in cases {
            let call = format!("{name} with {args}");
            assert_err_contains(run(name, args), expected, &call);
        }
        assert_eq!(tmp.contents("a"), "original");
    }

    #[test]
    fn run_err_when_working_directory_is_gone() {
        let tmp = TestDirs::new();
        fs::remove_dir(tmp.root()).expect("to remove root");

        assert!(list(".").is_err());
    }

    // read

    #[test]
    fn read_ok() {
        let tmp = TestDirs::new();
        tmp.create("docs/README.md", "hello");

        assert_eq!(read("docs/README.md").expect("read to succeed"), "hello");
    }

    #[test]
    fn read_ok_with_absolute_path_inside_root() {
        let tmp = TestDirs::new();
        tmp.create("README.md", "hello");

        let path = tmp.join("README.md");

        assert_eq!(read(path_str(&path)).expect("read to succeed"), "hello");
    }

    #[test]
    fn read_ok_through_symlink_inside_root() {
        let tmp = TestDirs::new();
        tmp.create("README.md", "hello");
        symlink("README.md", tmp.join("link.md")).expect("to create the symlink");

        assert_eq!(read("link.md").expect("read to succeed"), "hello");
    }

    #[test]
    fn read_err_when_path_is_outside_root() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        let absolute = tmp.outside("secret");
        for path in [
            "../outside/secret",
            "sub/../../outside/secret",
            path_str(&absolute),
        ] {
            assert_err_contains(read(path), "access denied", path);
        }
    }

    #[test]
    fn read_err_through_symlink_to_file_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside("secret"), tmp.join("link")).expect("to create the symlink");

        assert_err_contains(read("link"), "access denied", "link");
    }

    #[test]
    fn read_err_through_symlinked_parent_dir_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside(""), tmp.join("linkdir")).expect("to create the symlink");

        assert_err_contains(read("linkdir/secret"), "access denied", "linkdir/secret");
    }

    #[test]
    fn read_err_through_dangling_symlink() {
        let tmp = TestDirs::new();
        symlink(tmp.outside("missing"), tmp.join("link")).expect("to create the symlink");

        assert!(read("link").is_err());
    }

    #[test]
    fn read_err_when_file_is_missing() {
        let _tmp = TestDirs::new();

        assert!(read("missing.md").is_err());
    }

    #[test]
    fn read_err_when_path_is_a_dir() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        assert_err_contains(read("sub"), "cannot read", "sub");
    }

    #[test]
    fn read_err_when_file_is_not_utf8() {
        let tmp = TestDirs::new();
        fs::write(tmp.join("binary"), [0xff, 0xfe]).expect("to create the binary file");

        assert_err_contains(read("binary"), "cannot read", "binary");
    }

    // write

    #[test]
    fn write_ok_creates_file() {
        let tmp = TestDirs::new();

        let result = write("summary.md", "hello").expect("write to succeed");

        assert_eq!(result, "successfully wrote file");
        assert_eq!(tmp.contents("summary.md"), "hello");
    }

    #[test]
    fn write_ok_into_subdir() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("docs")).expect("to create docs");

        write("docs/summary.md", "hello").expect("write to succeed");

        assert_eq!(tmp.contents("docs/summary.md"), "hello");
    }

    #[test]
    fn write_ok_overwrites_with_shorter_contents() {
        let tmp = TestDirs::new();
        tmp.create("summary.md", "a much longer original text");

        write("summary.md", "short").expect("write to succeed");

        assert_eq!(tmp.contents("summary.md"), "short");
    }

    #[test]
    fn write_ok_with_leading_dot() {
        let tmp = TestDirs::new();

        write("./foo", "hello").expect("writing './foo' to succeed");

        assert_eq!(tmp.contents("foo"), "hello");
    }

    #[test]
    fn write_ok_with_dot_in_the_middle() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        write("sub/./foo", "hello").expect("writing 'sub/./foo' to succeed");

        assert_eq!(tmp.contents("sub/foo"), "hello");
    }

    #[test]
    fn write_err_when_path_has_no_file_name() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        for path in ["", ".", "..", "/", "sub/.."] {
            assert_err_contains(write(path, "hello"), "invalid path", path);
        }
    }

    #[test]
    fn write_err_when_path_ends_with_slash_or_dot() {
        let tmp = TestDirs::new();

        for path in ["foo/", "foo/.", "foo//", "foo/./"] {
            assert_err_contains(write(path, "hello"), "invalid path", path);
            assert!(!tmp.join("foo").exists(), "'{path}' created foo");
        }
    }

    #[test]
    fn write_err_when_nested_path_ends_with_slash() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        assert_err_contains(write("sub/foo/", "hello"), "invalid path", "sub/foo/");

        assert!(!tmp.join("sub/foo").exists());
    }

    #[test]
    fn write_err_when_path_with_trailing_slash_names_existing_file() {
        let tmp = TestDirs::new();
        tmp.create("notes.md", "original");

        assert_err_contains(
            write("notes.md/", "overwritten"),
            "invalid path",
            "notes.md/",
        );

        assert_eq!(tmp.contents("notes.md"), "original");
    }

    #[test]
    fn write_err_when_path_with_trailing_slash_names_existing_dir() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("foo")).expect("to create foo");

        assert_err_contains(write("foo/", "hello"), "invalid path", "foo/");

        assert!(tmp.join("foo").is_dir());
    }

    #[test]
    fn write_err_when_parent_dir_is_missing() {
        let tmp = TestDirs::new();

        assert_err_contains(write("nope/f", "hello"), "cannot write 'nope/f'", "nope/f");

        assert!(!tmp.join("nope").exists());
    }

    #[test]
    fn write_err_when_path_is_outside_root() {
        let tmp = TestDirs::new();
        fs::create_dir(tmp.join("sub")).expect("to create sub");

        let absolute = tmp.outside("new");
        for path in [
            "../outside/new",
            "sub/../../outside/new",
            path_str(&absolute),
        ] {
            assert_err_contains(write(path, "hello"), "access denied", path);
        }
        assert!(!tmp.outside("new").exists());
    }

    #[test]
    fn write_err_through_symlink_to_file_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside("secret"), tmp.join("link")).expect("to create the symlink");

        assert_err_contains(write("link", "overwritten"), "cannot write", "link");

        assert_eq!(tmp.secret(), "secret");
    }

    // without O_NOFOLLOW, O_CREAT would follow the symlink and create the target outside of root
    #[test]
    fn write_err_through_dangling_symlink_to_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside("new"), tmp.join("link")).expect("to create the symlink");

        assert_err_contains(write("link", "hello"), "cannot write", "link");

        assert!(!tmp.outside("new").exists());
    }

    #[test]
    fn write_err_through_symlinked_parent_dir_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside(""), tmp.join("linkdir")).expect("to create the symlink");

        assert_err_contains(
            write("linkdir/new", "hello"),
            "access denied",
            "linkdir/new",
        );

        assert!(!tmp.outside("new").exists());
    }

    // list

    #[test]
    fn list_ok_sorted_by_path_components() {
        let tmp = TestDirs::new();
        for path in ["b.txt", "c/d/e", "a.txt", "A.txt", "a/b"] {
            tmp.create(path, "");
        }

        // components are compared one by one, so "a/b" sorts before "a.txt" although '.' < '/'
        assert_eq!(
            listed(&tmp, "."),
            ["A.txt", "a/b", "a.txt", "b.txt", "c/d/e"]
        );
    }

    #[test]
    fn list_ok_with_subdir() {
        let tmp = TestDirs::new();
        tmp.create("top.txt", "");
        tmp.create("sub/inner.txt", "");

        assert_eq!(listed(&tmp, "sub"), ["sub/inner.txt"]);
    }

    #[test]
    fn list_ok_with_empty_path_lists_root() {
        let tmp = TestDirs::new();
        tmp.create("top.txt", "");

        assert_eq!(listed(&tmp, ""), ["top.txt"]);
    }

    #[test]
    fn list_ok_skips_names_from_skip_list_at_any_depth() {
        let tmp = TestDirs::new();
        for path in [
            ".git/config",
            "target/debug/regslop",
            ".env",
            "sub/.git/HEAD",
            "sub/target/x",
            "sub/.env",
            ".envrc",
            ".gitignore",
            ".github/workflows/ci.yml",
            "env",
            "sub/file",
        ] {
            tmp.create(path, "");
        }

        assert_eq!(
            listed(&tmp, "."),
            [
                ".envrc",
                ".github/workflows/ci.yml",
                ".gitignore",
                "env",
                "sub/file"
            ]
        );
    }

    #[test]
    fn list_ok_with_non_utf8_names() {
        let tmp = TestDirs::new();
        tmp.create(OsStr::from_bytes(b"\xff.txt"), "");
        tmp.create(Path::new(OsStr::from_bytes(b"\xfe")).join("file"), "");
        tmp.create(Path::new(OsStr::from_bytes(b"\xfe")).join(".git/HEAD"), "");

        // display() replaces invalid UTF-8 with U+FFFD, the sort order is by the raw bytes
        assert_eq!(listed(&tmp, "."), ["\u{FFFD}/file", "\u{FFFD}.txt"]);
    }

    #[test]
    fn list_ok_does_not_follow_symlinked_dirs() {
        let tmp = TestDirs::new();
        tmp.create("file.txt", "");
        symlink(tmp.outside(""), tmp.join("outside_link")).expect("to create the symlink");
        symlink(".", tmp.join("loop")).expect("to create the symlink loop");

        assert_eq!(listed(&tmp, "."), ["file.txt", "loop", "outside_link"]);
    }

    #[test]
    fn list_ok_with_empty_dir_returns_empty_string() {
        let _tmp = TestDirs::new();

        assert_eq!(list(".").expect("listing to succeed"), "");
    }

    #[test]
    fn list_err_when_path_is_outside_root() {
        let tmp = TestDirs::new();
        symlink(tmp.outside(""), tmp.join("linkdir")).expect("to create the symlink");

        let absolute = tmp.outside("");
        for path in ["..", "../outside", "linkdir", path_str(&absolute)] {
            assert_err_contains(list(path), "access denied", path);
        }
    }

    #[test]
    fn list_err_when_path_is_a_file() {
        let tmp = TestDirs::new();
        tmp.create("file.txt", "");

        assert!(list("file.txt").is_err());
    }

    #[test]
    fn list_err_when_dir_is_missing() {
        let _tmp = TestDirs::new();

        assert!(list("missing").is_err());
    }
}
