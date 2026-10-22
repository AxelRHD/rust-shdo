//! Syncing via the `git` binary.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

pub struct Git<'a> {
    dir: &'a Path,
    /// Without a terminal (TUI): no password/passphrase prompts, otherwise everything hangs.
    batch: bool,
}

impl<'a> Git<'a> {
    pub fn new(dir: &'a Path, batch: bool) -> Git<'a> {
        Git { dir, batch }
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new("git");
        cmd.arg("-C").arg(self.dir).args(args);
        if self.batch {
            cmd.env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
            if std::env::var_os("GIT_SSH_COMMAND").is_none() {
                cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
            }
        }
        cmd
    }

    /// Runs git and returns stdout; on failure the full output.
    pub fn run(&self, args: &[&str]) -> Result<String> {
        let out = self
            .command(args)
            .output()
            .context("git konnte nicht gestartet werden – ist git installiert?")?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            bail!(
                "git {} fehlgeschlagen:\n{}\n{}",
                args.join(" "),
                stdout,
                stderr
            );
        }
        Ok(stdout)
    }

    pub fn is_repo(&self) -> bool {
        self.dir.join(".git").exists()
    }

    pub fn init(&self) -> Result<()> {
        self.run(&["init", "-q"]).map(|_| ())
    }

    /// add, commit (if needed), pull --rebase, push. Returns a summary.
    pub fn sync(&self) -> Result<String> {
        if !self.is_repo() {
            bail!(
                "{} ist kein Git-Repo – `shdo init` legt eins an",
                self.dir.display()
            );
        }
        let mut report = Vec::new();
        self.run(&["add", "-A"])?;
        if self.run(&["status", "--porcelain"])?.is_empty() {
            report.push("nichts zu committen".to_string());
        } else {
            let msg = format!(
                "shdo sync {} {}",
                hostname(),
                jiff::Zoned::now().strftime("%Y-%m-%d %H:%M")
            );
            self.run(&["commit", "-q", "-m", &msg])?;
            report.push("committet".to_string());
        }
        let remotes = self.run(&["remote"])?;
        let Some(remote) = remotes.lines().next() else {
            report.push("kein Remote konfiguriert, nur lokal".to_string());
            return Ok(report.join(", "));
        };
        let has_upstream = self
            .run(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"])
            .is_ok();
        if has_upstream {
            self.run(&["pull", "--rebase", "-q"])?;
            report.push("gepullt".to_string());
            self.run(&["push", "-q"])?;
        } else {
            self.run(&["push", "-q", "-u", remote, "HEAD"])?;
        }
        report.push(format!("gepusht nach {remote}"));
        Ok(report.join(", "))
    }
}

fn hostname() -> String {
    std::fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "unbekannt".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn git_available() -> bool {
        Command::new("git").arg("--version").output().is_ok()
    }

    fn configure(dir: &Path) {
        let g = Git::new(dir, true);
        g.run(&["config", "user.email", "test@example.com"])
            .unwrap();
        g.run(&["config", "user.name", "Test"]).unwrap();
        g.run(&["config", "commit.gpgsign", "false"]).unwrap();
    }

    #[test]
    fn sync_local_and_with_remote() {
        if !git_available() {
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let remote = tmp.path().join("remote.git");
        let a = tmp.path().join("a");
        fs::create_dir_all(&a).unwrap();
        Git::new(tmp.path(), true)
            .run(&["init", "-q", "--bare", remote.to_str().unwrap()])
            .unwrap();

        let ga = Git::new(&a, true);
        ga.init().unwrap();
        configure(&a);
        fs::write(a.join("x.md"), "eins").unwrap();
        let r = ga.sync().unwrap();
        assert!(r.contains("committet") && r.contains("kein Remote"), "{r}");
        assert!(ga.sync().unwrap().contains("nichts zu committen"));

        ga.run(&["remote", "add", "origin", remote.to_str().unwrap()])
            .unwrap();
        let r = ga.sync().unwrap();
        assert!(r.contains("gepusht nach origin"), "{r}");

        // A second machine clones, changes and syncs; the first one picks it up.
        let b = tmp.path().join("b");
        Git::new(tmp.path(), true)
            .run(&["clone", "-q", remote.to_str().unwrap(), b.to_str().unwrap()])
            .unwrap();
        configure(&b);
        fs::write(b.join("y.md"), "zwei").unwrap();
        Git::new(&b, true).sync().unwrap();
        fs::write(a.join("z.md"), "drei").unwrap();
        let r = ga.sync().unwrap();
        assert!(r.contains("gepullt"), "{r}");
        assert!(a.join("y.md").exists());
    }

    #[test]
    fn no_repo_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Git::new(tmp.path(), true).sync().is_err());
    }
}
