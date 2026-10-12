//! scripts/release.sh in a throwaway repo with a local bare remote: the guards that make
//! `make release` safe (never amends a pushed commit, never forces) and the happy path.
use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
};

struct Sandbox {
    _dir: tempfile::TempDir,
    work: PathBuf,
    remote: PathBuf,
}

fn git(dir: &Path, args: &[&str]) -> Output {
    let o = Command::new("git")
        .current_dir(dir)
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "init.defaultBranch=main",
        ])
        .args(args)
        .output()
        .expect("git");
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o
}

fn out(o: Output) -> String {
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

impl Sandbox {
    fn new() -> Sandbox {
        let dir = tempfile::tempdir().unwrap();
        let remote = dir.path().join("remote.git");
        let work = dir.path().join("work");
        std::fs::create_dir_all(&remote).unwrap();
        std::fs::create_dir_all(&work).unwrap();
        git(&remote, &["init", "-q", "--bare"]);
        git(&work, &["init", "-q"]);
        git(
            &work,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        std::fs::create_dir_all(work.join("scripts")).unwrap();
        std::fs::copy(
            concat!(env!("CARGO_MANIFEST_DIR"), "/scripts/release.sh"),
            work.join("scripts/release.sh"),
        )
        .unwrap();
        std::fs::write(work.join("VERSION"), "0.1.0\n").unwrap();
        std::fs::create_dir_all(work.join("RELEASE")).unwrap();
        std::fs::write(work.join("RELEASE/v0.1.0.md"), "# v0.1.0\n").unwrap();
        git(&work, &["add", "-A"]);
        git(&work, &["commit", "-q", "-m", "init"]);
        git(&work, &["push", "-q", "-u", "origin", "main"]);
        Sandbox {
            _dir: dir,
            work,
            remote,
        }
    }

    fn commit(&self, file: &str, msg: &str) {
        std::fs::write(self.work.join(file), msg).unwrap();
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-q", "-m", msg]);
    }

    fn notes(&self, v: &str) {
        std::fs::write(
            self.work.join(format!("RELEASE/v{v}.md")),
            format!("# v{v}\n"),
        )
        .unwrap();
        git(&self.work, &["add", "-A"]);
        git(&self.work, &["commit", "-q", "--amend", "--no-edit"]);
    }

    fn release(&self, v: &str, env: &[(&str, &str)]) -> Output {
        let mut c = Command::new("bash");
        c.current_dir(&self.work)
            .args(["scripts/release.sh", v])
            .env("SKIP_CHECK", "1");
        for (k, val) in env {
            c.env(k, val);
        }
        c.output().unwrap()
    }

    fn remote_has_tag(&self, t: &str) -> bool {
        !out(git(&self.remote, &["tag", "-l", t])).is_empty()
    }
}

fn err(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).to_string()
}

#[test]
fn happy_path_bumps_amends_pushes_and_tags_one_commit() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    s.notes("0.2.0");
    let before = out(git(&s.work, &["rev-parse", "HEAD"]));
    let o = s.release("0.2.0", &[]);
    assert!(o.status.success(), "{}", err(&o));
    assert_eq!(
        std::fs::read_to_string(s.work.join("VERSION"))
            .unwrap()
            .trim(),
        "0.2.0"
    );
    let head = out(git(&s.work, &["rev-parse", "HEAD"]));
    assert_ne!(head, before, "bump was amended into the new commit");
    assert_eq!(
        out(git(&s.remote, &["rev-parse", "main"])),
        head,
        "remote main is HEAD, no force needed"
    );
    assert!(s.remote_has_tag("v0.2.0"));
    assert_eq!(
        out(git(&s.work, &["rev-parse", "v0.2.0"])),
        head,
        "tag points at the released commit"
    );
    assert_eq!(
        out(git(&s.work, &["rev-list", "--count", "origin/main..HEAD"])),
        "0"
    );
}

#[test]
fn refuses_when_head_is_already_pushed_instead_of_amending_it() {
    let s = Sandbox::new();
    s.notes("0.2.0"); // new notes file amended into the pushed commit locally? no: commit it properly
    git(&s.work, &["push", "-q", "-f", "origin", "main"]); // make the sandbox state "HEAD already on origin"
    let o = s.release("0.2.0", &[]);
    assert!(!o.status.success());
    assert!(err(&o).contains("already on"), "{}", err(&o));
    assert!(!s.remote_has_tag("v0.2.0"));
}

#[test]
fn refuses_a_dirty_working_tree() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    s.notes("0.2.0");
    std::fs::write(s.work.join("scratch.txt"), "x").unwrap();
    let o = s.release("0.2.0", &[]);
    assert!(!o.status.success());
    assert!(err(&o).contains("not clean"), "{}", err(&o));
}

#[test]
fn refuses_an_existing_tag_locally_and_remotely() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    s.notes("0.2.0");
    git(&s.work, &["tag", "v0.2.0"]);
    let o = s.release("0.2.0", &[]);
    assert!(
        !o.status.success() && err(&o).contains("already exists"),
        "{}",
        err(&o)
    );
    git(&s.work, &["tag", "-d", "v0.2.0"]);
    git(&s.remote, &["tag", "v0.2.0", "main"]);
    let o = s.release("0.2.0", &[]);
    assert!(
        !o.status.success() && err(&o).contains("already exists"),
        "{}",
        err(&o)
    );
}

#[test]
fn refuses_when_the_release_notes_file_is_missing() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    let o = s.release("0.2.0", &[]);
    assert!(!o.status.success());
    assert!(
        err(&o).contains("RELEASE/v0.2.0.md is missing"),
        "{}",
        err(&o)
    );
}

#[test]
fn refuses_when_behind_or_diverged_and_never_forces() {
    let s = Sandbox::new();
    // someone else pushes to origin/main
    let other = s.work.parent().unwrap().join("other");
    git(
        s.work.parent().unwrap(),
        &[
            "clone",
            "-q",
            s.remote.to_str().unwrap(),
            other.to_str().unwrap(),
        ],
    );
    std::fs::write(other.join("o.txt"), "o").unwrap();
    git(&other, &["add", "-A"]);
    git(&other, &["commit", "-q", "-m", "other work"]);
    git(&other, &["push", "-q", "origin", "main"]);
    s.commit("a.txt", "feat: a");
    s.notes("0.2.0");
    let o = s.release("0.2.0", &[]);
    assert!(!o.status.success());
    assert!(err(&o).contains("behind (or diverged"), "{}", err(&o));
    assert!(!s.remote_has_tag("v0.2.0"));
}

#[test]
fn rejects_malformed_versions() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    for bad in ["1.2", "v1.2.3", "1.2.3-rc1", "x"] {
        let o = s.release(bad, &[]);
        assert!(
            !o.status.success() && err(&o).contains("must look like"),
            "{bad}: {}",
            err(&o)
        );
    }
}

#[test]
fn dry_run_checks_everything_and_changes_nothing() {
    let s = Sandbox::new();
    s.commit("a.txt", "feat: a");
    s.notes("0.2.0");
    let head = out(git(&s.work, &["rev-parse", "HEAD"]));
    let o = s.release("0.2.0", &[("DRY_RUN", "1")]);
    assert!(o.status.success(), "{}", err(&o));
    assert!(String::from_utf8_lossy(&o.stdout).contains("(dry-run)"));
    assert_eq!(out(git(&s.work, &["rev-parse", "HEAD"])), head);
    assert_eq!(
        std::fs::read_to_string(s.work.join("VERSION"))
            .unwrap()
            .trim(),
        "0.1.0"
    );
    assert!(!s.remote_has_tag("v0.2.0"));
}
