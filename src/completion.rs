//! Shell completion: static via `clap_complete`, plus dynamic suggestions for fish
//! (IDs with title, statuses, buckets, `due:` values) via the hidden `__complete` command.

use std::io::Write;

use anyhow::Result;
use clap::ValueEnum;
use clap_complete::Shell;

use crate::board::{Board, Filter};
use crate::dates;
use crate::store::Store;

#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    /// IDs of open and done tasks, title as description
    Ids,
    /// Status keys
    Statuses,
    /// Bucket keys plus "-" for removal
    Buckets,
    /// Buckets as `@key` (quick add, `ls`)
    AtBuckets,
    /// `due:` shortcuts with the resolved date
    Due,
}

const FISH_DYNAMIC: &str = r#"
# --- dynamische Vorschläge (shdo) ---
# shdo nimmt nur bei --dir Pfade entgegen, sonst nie Dateinamen vorschlagen
complete -c shdo -f
complete -c shdo -l dir -x -a '(__fish_complete_directories (commandline -ct))'

function __shdo_nargs --description 'Positionsargumente inkl. Befehl, ohne --dir'
    set -l toks (commandline -opc)
    set -e toks[1]
    set -l n 0
    set -l skip 0
    for t in $toks
        if test $skip -eq 1
            set skip 0
            continue
        end
        switch $t
            case --dir
                set skip 1
            case '-*'
            case '*'
                set n (math $n + 1)
        end
    end
    echo $n
end

function __shdo_arg --description '__shdo_arg <pos> <befehl…>: Cursor steht auf Argument <pos>'
    set -l pos $argv[1]
    set -e argv[1]
    __fish_seen_subcommand_from $argv; and test (__shdo_nargs) -eq $pos
end

function __shdo_complete --description 'shdo __complete mit dem --dir der Kommandozeile'
    set -l toks (commandline -opc)
    set -l dir
    if set -l i (contains -i -- --dir $toks); and set -q toks[(math $i + 1)]
        set dir --dir $toks[(math $i + 1)]
    end
    shdo $dir __complete $argv 2>/dev/null
end

complete -c shdo -n '__shdo_arg 1 done rm edit mv bucket' -f -a '(__shdo_complete ids)'
complete -c shdo -n '__shdo_arg 2 mv' -f -a '(__shdo_complete statuses)'
complete -c shdo -n '__shdo_arg 2 bucket' -f -a '(__shdo_complete buckets)'
complete -c shdo -n '__shdo_arg 1 ls' -f -a '(__shdo_complete at-buckets)'
complete -c shdo -n 'string match -q -- "@*" (commandline -ct)' -f -a '(__shdo_complete at-buckets)'
complete -c shdo -n 'string match -q -- "due:*" (commandline -ct)' -f -a '(__shdo_complete due)'
"#;

pub const SETUP_HELP: &str = "\
Einrichten:
  fish  shdo completions fish > ~/.config/fish/completions/shdo.fish
  bash  shdo completions bash > ~/.local/share/bash-completion/completions/shdo
  zsh   shdo completions zsh > ~/.zfunc/_shdo
        (in ~/.zshrc vor compinit: fpath+=~/.zfunc)

Danach eine neue Shell öffnen. Nach einem Update von shdo den Befehl
erneut ausführen, damit neue Befehle und Optionen dabei sind.

Fish bekommt zusätzlich dynamische Vorschläge: Task-IDs mit Titel,
Status, Buckets (@…) und due:-Werte mit Datum. Bash, Zsh, Elvish und
PowerShell kennen Befehle und Optionen.";

/// Guide for `shdo completions` without a shell, pointing at the user's shell.
pub fn setup_guide() -> String {
    let mut out = format!(
        "Gibt ein Completion-Skript für die Shell aus.\n\n\
         Aufruf: shdo completions <SHELL>\n\
         Shells: bash, elvish, fish, powershell, zsh\n\n{SETUP_HELP}\n"
    );
    let own = std::env::var("SHELL")
        .ok()
        .and_then(|s| s.rsplit('/').next().map(str::to_string))
        .and_then(|name| Shell::from_str(&name, true).ok());
    if let Some(shell) = own {
        out.push_str(&format!(
            "\nDeine Shell laut $SHELL: {shell} → shdo completions {shell}\n"
        ));
    }
    out
}

pub fn print_script(shell: Shell, cmd: &mut clap::Command) {
    let mut buf = Vec::new();
    clap_complete::generate(shell, cmd, "shdo", &mut buf);
    let mut out = std::io::stdout();
    if shell != Shell::Fish {
        let _ = out.write_all(&buf);
        return;
    }
    // clap_complete also lists the hidden command; drop those lines
    let script = String::from_utf8_lossy(&buf);
    for line in script.lines().filter(|l| !l.contains("__complete")) {
        let _ = writeln!(out, "{line}");
    }
    let _ = out.write_all(FISH_DYNAMIC.as_bytes());
    let shells: Vec<String> = Shell::value_variants()
        .iter()
        .filter_map(|s| s.to_possible_value().map(|v| v.get_name().to_string()))
        .collect();
    let _ = writeln!(
        out,
        "complete -c shdo -n '__shdo_arg 1 completions' -f -a '{}'",
        shells.join(" ")
    );
}

/// Suggestions as `value\tdescription`, one line per entry.
pub fn candidates(store: &Store, kind: Kind) -> Result<Vec<String>> {
    let lines = match kind {
        Kind::Ids => {
            let board = Board::load(store, &Filter::all())?;
            board
                .columns
                .iter()
                .flat_map(|c| {
                    c.tasks
                        .iter()
                        .map(move |t| format!("{}\t{} ({})", t.id, t.title, c.def.name))
                })
                .collect()
        }
        Kind::Statuses => store
            .board_config()?
            .statuses
            .iter()
            .map(|s| format!("{}\t{}", s.key, s.name))
            .collect(),
        Kind::Buckets => {
            let mut v: Vec<String> = store
                .board_config()?
                .buckets
                .iter()
                .map(|b| format!("{}\t{}", b.key, b.name))
                .collect();
            v.push("-\tohne Bucket".into());
            v
        }
        Kind::AtBuckets => store
            .board_config()?
            .buckets
            .iter()
            .map(|b| format!("@{}\t{}", b.key, b.name))
            .collect(),
        Kind::Due => due_candidates(dates::today()),
    };
    Ok(lines)
}

fn due_candidates(today: jiff::civil::Date) -> Vec<String> {
    [
        "heute",
        "morgen",
        "übermorgen",
        "mo",
        "di",
        "mi",
        "do",
        "fr",
        "sa",
        "so",
        "+1w",
        "+2w",
        "+1m",
    ]
    .iter()
    .filter_map(|s| {
        let d = dates::parse_date(s, today).ok()?;
        Some(format!("due:{s}\t{}", dates::fmt_long(d)))
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn due_candidates_resolve_dates() {
        let c = due_candidates(date(2026, 10, 22));
        assert!(c.contains(&"due:fr\tfr 23.10.2026".to_string()), "{c:?}");
        assert!(c.iter().all(|l| l.starts_with("due:") && l.contains('\t')));
    }

    #[test]
    fn candidates_from_store() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::new(tmp.path().to_path_buf());
        store.init().unwrap();
        let t = store.create("Rechnung", "todo", &jiff::Zoned::now());
        t.save().unwrap();
        let ids = candidates(&store, Kind::Ids).unwrap();
        assert_eq!(ids, [format!("{}\tRechnung (Offen)", t.id)]);
        let st = candidates(&store, Kind::Statuses).unwrap();
        assert_eq!(st[0], "todo\tOffen");
        assert_eq!(
            candidates(&store, Kind::Buckets).unwrap(),
            ["-\tohne Bucket"]
        );
        assert!(candidates(&store, Kind::AtBuckets).unwrap().is_empty());
    }
}
