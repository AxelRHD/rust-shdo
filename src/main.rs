mod board;
mod completion;
mod config;
mod dates;
mod git;
mod holidays;
mod quickadd;
mod store;
mod task;
mod theme;
mod tui;

use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};
use clap::{CommandFactory, Parser, Subcommand};

use board::{Board, Filter};
use config::AppConfig;
use store::Store;

#[derive(Parser)]
#[command(
    name = "shdo",
    version,
    disable_help_subcommand = true,
    about = "Kanban-Board fürs Terminal – Tasks als Markdown-Dateien in einem Git-Repo",
    after_help = "Ohne Argumente öffnet sich das Board.\n\
                  Neuer Task: shdo Rechnung bezahlen P1 due:fr\n\
                  Beginnt der Titel mit einem Befehl: shdo -- ls reparieren"
)]
struct Cli {
    /// Datenverzeichnis (statt `dir` aus der Config bzw. ~/tasks)
    #[arg(long, global = true, value_name = "PFAD")]
    dir: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Neuen Task anlegen (wie `shdo <text>`)
    #[command(visible_alias = "a")]
    Add {
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        text: Vec<String>,
    },
    /// Board als Text ausgeben (beachtet den Bucket-Filter der TUI)
    Ls {
        /// Alle Buckets anzeigen
        #[arg(short, long)]
        all: bool,
        /// Nur diesen Bucket anzeigen (z. B. @work)
        bucket: Option<String>,
    },
    /// Bucket eines Tasks setzen („-“ entfernt ihn)
    Bucket { id: String, bucket: String },
    /// Task als erledigt markieren (letzter Status)
    Done { id: String },
    /// Status eines Tasks setzen (key oder Name, Präfix reicht)
    Mv { id: String, status: String },
    /// Task löschen
    Rm { id: String },
    /// Task-Datei im Editor öffnen
    Edit { id: String },
    /// Mit dem Git-Remote synchronisieren
    Sync,
    /// Erledigte Tasks nach archive/ verschieben
    Archive,
    /// Datenverzeichnis anlegen (inkl. git init)
    Init,
    /// Completion-Skript für die Shell ausgeben (ohne Argument: Anleitung)
    #[command(after_help = completion::SETUP_HELP)]
    Completions {
        /// Shell, für die das Skript erzeugt wird
        shell: Option<clap_complete::Shell>,
    },
    /// Intern: dynamische Vorschläge für die Completion
    #[command(name = "__complete", hide = true)]
    Complete { kind: completion::Kind },
    /// Alles andere: `shdo Rechnung bezahlen P1 due:fr`
    #[command(external_subcommand)]
    Text(Vec<String>),
}

const SUBCOMMANDS: &[&str] = &[
    "add",
    "ls",
    "done",
    "mv",
    "rm",
    "edit",
    "sync",
    "archive",
    "init",
    "bucket",
    "completions",
];

fn main() {
    if let Err(e) = run() {
        eprintln!("Fehler: {e:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse_from(rewrite_dashdash(std::env::args().collect()));
    let cfg_path = config::config_path();
    let cfg = AppConfig::load(&cfg_path)?;
    let store = Store::new(cfg.data_dir(cli.dir.as_deref()));
    let filter = Filter {
        buckets: cfg.buckets.clone(),
    };
    let all = Filter::all();

    let Some(cmd) = cli.cmd else {
        store.ensure_initialized()?;
        return tui::run(store, cfg, cfg_path);
    };
    match &cmd {
        Cmd::Text(words) => guard_typo(words)?,
        Cmd::Completions { shell: Some(shell) } => {
            completion::print_script(*shell, &mut Cli::command());
            return Ok(());
        }
        Cmd::Completions { shell: None } => {
            print!("{}", completion::setup_guide());
            return Ok(());
        }
        // Completion soll nie mit Fehlern stören
        Cmd::Complete { kind } => {
            for line in completion::candidates(&store, *kind).unwrap_or_default() {
                println!("{line}");
            }
            return Ok(());
        }
        _ => {}
    }
    if !matches!(cmd, Cmd::Init) {
        store.ensure_initialized()?;
    }
    match cmd {
        Cmd::Add { text } | Cmd::Text(text) => add(&store, &filter, &text),
        Cmd::Ls { all: true, .. } => ls(&store, &all),
        Cmd::Ls {
            bucket: Some(b), ..
        } => {
            let cfg = store.board_config()?;
            let key = board::resolve_bucket(&cfg.buckets, &b)?;
            ls(&store, &Filter { buckets: vec![key] })
        }
        Cmd::Ls { .. } => ls(&store, &filter),
        Cmd::Bucket { id, bucket } => {
            let mut task = store.find(&id)?;
            let cfg = store.board_config()?;
            let key = match bucket.as_str() {
                "-" => None,
                b => Some(board::resolve_bucket(&cfg.buckets, b)?),
            };
            task.set_bucket(key.as_deref());
            task.save()?;
            match key {
                Some(k) => println!("{}  {}  @{k}", task.id, task.title),
                None => println!("{}  {}  ohne Bucket", task.id, task.title),
            }
            Ok(())
        }
        Cmd::Done { id } => {
            let board = Board::load(&store, &all)?;
            move_cmd(&store, &board, &id, board.last_index())
        }
        Cmd::Mv { id, status } => {
            let board = Board::load(&store, &all)?;
            let target = board.resolve_status(&status).with_context(|| {
                let names: Vec<_> = board.columns.iter().map(|c| c.def.key.as_str()).collect();
                format!(
                    "unbekannter Status „{status}“ (vorhanden: {})",
                    names.join(", ")
                )
            })?;
            move_cmd(&store, &board, &id, target)
        }
        Cmd::Rm { id } => {
            let task = store.find(&id)?;
            store.delete(&task)?;
            println!("gelöscht {}  {}", task.id, task.title);
            Ok(())
        }
        Cmd::Edit { id } => {
            let task = store.find(&id)?;
            open_editor(&cfg.editor(), &task.path)?;
            if let Err(e) = task::Task::load(&task.path) {
                eprintln!("Warnung: {} ist jetzt ungültig: {e:#}", task.path.display());
            }
            Ok(())
        }
        Cmd::Sync => {
            println!("{}", git::Git::new(&store.dir, false).sync()?);
            Ok(())
        }
        Cmd::Archive => {
            let board = Board::load(&store, &all)?;
            let last = &board.columns[board.last_index()];
            for t in &last.tasks {
                store.archive(t)?;
            }
            println!(
                "{} Task(s) aus „{}“ archiviert",
                last.tasks.len(),
                last.def.name
            );
            Ok(())
        }
        Cmd::Init => init(&store),
        // bereits vor der Initialisierungsprüfung behandelt
        Cmd::Completions { .. } | Cmd::Complete { .. } => Ok(()),
    }
}

/// `shdo -- ls reparieren` wird zu `shdo add ls reparieren`.
fn rewrite_dashdash(mut args: Vec<String>) -> Vec<String> {
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--dir" => i += 2,
            a if a.starts_with("--dir=") => i += 1,
            "--" => {
                args[i] = "add".into();
                break;
            }
            _ => break,
        }
    }
    args
}

/// `shdo sycn` soll keinen Task „sycn“ anlegen.
fn guard_typo(text: &[String]) -> Result<()> {
    let [word] = text else { return Ok(()) };
    let w = word.to_lowercase();
    if let Some(cmd) = SUBCOMMANDS.iter().find(|c| osa_distance(&w, c) <= 1) {
        bail!(
            "„{word}“ sieht aus wie der Befehl `{cmd}`.\n\
             Als Task anlegen: shdo add {word}\n\
             Hilfe: shdo --help"
        );
    }
    Ok(())
}

/// Editierdistanz mit Vertauschung benachbarter Zeichen (Optimal String Alignment).
fn osa_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let mut d = vec![vec![0usize; b.len() + 1]; a.len() + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    for (j, v) in d[0].iter_mut().enumerate() {
        *v = j;
    }
    for i in 1..=a.len() {
        for j in 1..=b.len() {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            d[i][j] = (d[i - 1][j] + 1)
                .min(d[i][j - 1] + 1)
                .min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                d[i][j] = d[i][j].min(d[i - 2][j - 2] + 1);
            }
        }
    }
    d[a.len()][b.len()]
}

fn add(store: &Store, filter: &Filter, text: &[String]) -> Result<()> {
    let q = quickadd::parse(text, dates::today())?;
    let cfg = store.board_config()?;
    let filter = filter.clone().normalized(&cfg.buckets);
    let (bucket, inherited) = board::bucket_for_new(q.bucket.as_deref(), &filter, &cfg.buckets)?;
    let mut task = store.create(&q.title, &cfg.statuses[0].key, &jiff::Zoned::now());
    task.set_prio(q.prio);
    task.set_due(q.due);
    task.set_bucket(bucket.as_deref());
    task.save()?;
    match (&bucket, inherited) {
        (Some(b), true) => println!("{}  {}  @{b} (aus Filter)", task.id, task.title),
        (Some(b), false) => println!("{}  {}  @{b}", task.id, task.title),
        (None, _) => println!("{}  {}", task.id, task.title),
    }
    Ok(())
}

fn move_cmd(store: &Store, board: &Board, id: &str, target: usize) -> Result<()> {
    let mut task = store.find(id)?;
    let defs: Vec<_> = board.columns.iter().map(|c| c.def.clone()).collect();
    board::move_task(&mut task, &defs, target, &jiff::Zoned::now())?;
    println!("{}  {} → {}", task.id, task.title, defs[target].name);
    Ok(())
}

fn ls(store: &Store, filter: &Filter) -> Result<()> {
    let board = Board::load(store, filter)?;
    let color = std::io::stdout().is_terminal();
    let paint = |code: &str, s: &str| {
        if color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    };
    let today = dates::today();
    let last = board.last_index();
    if let Some(label) = board.filter_label() {
        println!(
            "{}",
            paint(
                "33",
                &format!(
                    "Filter: {label} · {} ausgeblendet (alle: shdo ls -a)",
                    board.hidden
                )
            )
        );
        println!();
    }
    // Bucket pro Zeile nur zeigen, wenn mehr als einer sichtbar sein kann
    let show_bucket = !board.buckets.is_empty() && board.filter.inherited().is_none();
    for (ci, col) in board.columns.iter().enumerate() {
        if ci > 0 {
            println!();
        }
        println!("{} ({})", paint("1;34", &col.def.name), col.tasks.len());
        for t in &col.tasks {
            let prio = format!("P{}", t.prio_or_default());
            let prio = match t.prio_or_default() {
                _ if ci == last => paint("32", "✓ "),
                1 => paint("31", &prio),
                3 => paint("2", &prio),
                _ => prio,
            };
            let (when, overdue) = if ci == last {
                (
                    t.done
                        .or(t.mtime)
                        .map(|d| dates::fmt_ago(d, today))
                        .unwrap_or_default(),
                    false,
                )
            } else {
                (
                    t.due.map(|d| dates::fmt_due(d, today)).unwrap_or_default(),
                    t.due.is_some_and(|d| d < today),
                )
            };
            let when = format!("{when:<10}");
            let when = if overdue { paint("31", &when) } else { when };
            let bucket = match (&t.bucket, show_bucket) {
                (Some(b), true) => format!("  {}", paint("35", &format!("@{b}"))),
                _ => String::new(),
            };
            println!(
                "  {}  {prio}  {when}  {}{bucket}",
                paint("2", &format!("{:<6}", t.id)),
                t.title
            );
        }
    }
    for w in &board.warnings {
        eprintln!("{} {w}", paint("33", "Warnung:"));
    }
    Ok(())
}

fn init(store: &Store) -> Result<()> {
    for c in store.init()? {
        println!("angelegt  {c}");
    }
    let git = git::Git::new(&store.dir, false);
    if !git.is_repo() {
        git.init()?;
        println!("git init  {}", store.dir.display());
    }
    println!("bereit: {}", store.dir.display());
    Ok(())
}

pub fn open_editor(editor: &str, path: &std::path::Path) -> Result<()> {
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = Command::new(bin)
        .args(parts)
        .arg(path)
        .status()
        .with_context(|| format!("Editor „{bin}“ konnte nicht gestartet werden"))?;
    if !status.success() {
        bail!("Editor beendet mit {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typo_guard() {
        assert_eq!(osa_distance("sycn", "sync"), 1);
        assert_eq!(osa_distance("lss", "ls"), 1);
        for word in ["sycn", "lss", "Init", "archiv"] {
            assert!(guard_typo(&[word.into()]).is_err(), "{word}");
        }
        assert!(guard_typo(&["Einkaufen".into()]).is_ok());
        assert!(guard_typo(&["sync".into(), "Kalender".into()]).is_ok());
    }

    #[test]
    fn cli_parsing() {
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["shdo", "Rechnung", "bezahlen", "P1"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Text(t)) if t == ["Rechnung", "bezahlen", "P1"]));
        let cli = Cli::try_parse_from(["shdo", "--dir", "/x", "Zahnarzt", "due:+2w"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Text(t)) if t == ["Zahnarzt", "due:+2w"]));
        let args = rewrite_dashdash(
            ["shdo", "--dir", "/x", "--", "ls", "x"]
                .map(String::from)
                .to_vec(),
        );
        let cli = Cli::try_parse_from(args).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Add { text }) if text == ["ls", "x"]));
        let args = rewrite_dashdash(["shdo", "add", "--", "x"].map(String::from).to_vec());
        assert_eq!(args[1], "add");
        let cli = Cli::try_parse_from(["shdo", "mv", "a3", "doing"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Mv { .. })));
        let cli = Cli::try_parse_from(["shdo", "a", "Zahnarzt", "due:+2w"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Add { .. })));
        let cli = Cli::try_parse_from(["shdo", "--dir", "/tmp/x", "ls"]).unwrap();
        assert_eq!(cli.dir, Some(PathBuf::from("/tmp/x")));
        assert!(matches!(cli.cmd, Some(Cmd::Ls { .. })));
        let cli = Cli::try_parse_from(["shdo", "ls", "--dir", "/tmp/x"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Ls { .. })));
        assert!(Cli::try_parse_from(["shdo"]).unwrap().cmd.is_none());
        let cli = Cli::try_parse_from(["shdo", "ls", "-a"]).unwrap();
        assert!(matches!(
            cli.cmd,
            Some(Cmd::Ls {
                all: true,
                bucket: None
            })
        ));
        let cli = Cli::try_parse_from(["shdo", "ls", "@work"]).unwrap();
        assert!(matches!(cli.cmd, Some(Cmd::Ls { bucket: Some(b), .. }) if b == "@work"));
    }
}
