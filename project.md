# shdo – Spezifikation

`shdo` („shell todo“) ist ein minimalistischer Terminal-Task-Manager mit Kanban-Board.
Tasks liegen als einzelne Markdown-Dateien mit TOML-Frontmatter in einem Git-Repo und
werden über Git zwischen mehreren Rechnern synchronisiert.

Abgrenzung: dstask ist ein Befehlsgenerator ohne „Programm“, tiki ist für den Zweck
überdimensioniert. shdo ist ein Programm, das mit dem Board startet und immer zeigt,
welche Tasten es gibt. Die CLI dient der schnellen Erfassung und Skripten.

## Ziele

- Programmstart direkt ins Kanban-Board, Tastenleiste permanent sichtbar
- Schnelles Erfassen von Tasks direkt aus der Shell, ohne die TUI zu öffnen
- Daten sind von Hand lesbar und editierbar und erzeugen saubere Git-Diffs
- Merge-Konflikte beim Sync sind praktisch ausgeschlossen (eine Datei pro Task)
- Bewusst klein gehalten, Oberfläche auf Deutsch

## Nicht-Ziele

- Keine Assignees, Projekte, Tags, Abhängigkeiten, Story Points, Recurrence, Uhrzeiten
- Kein Server, keine Datenbank, kein Netzwerkzugriff außer über `git`
- Kein manuelles Umsortieren (Reihenfolge ergibt sich automatisch)

## Umgebung

- Linux und WSL, headless, läuft in Zellij
- Editor: `editor` aus der Config, sonst `$VISUAL`, sonst `$EDITOR` (typischerweise `hx`)
- Shell des Nutzers: Fish

## Stack

- Rust stable, Edition 2024
- `ratatui` (inkl. re-exportiertem `crossterm`) für die TUI
- `clap` (derive) für die CLI
- `toml_edit` zum Schreiben (Kommentare/Formatierung bleiben erhalten), `toml` + `serde` zum Lesen von Configs
- `jiff` für Datum/Zeit
- `fastrand` für IDs, `anyhow` für Fehler, `unicode-width` für Kürzungen
- `git` über `std::process::Command`, keine libgit2-Abhängigkeit
- Dev: `tempfile`

## Konfiguration

### App-Config (rechnerlokal, nicht synchronisiert)

`$XDG_CONFIG_HOME/shdo/config.toml`, Fallback `~/.config/shdo/config.toml`. Fehlt die Datei,
gelten Defaults.

```toml
dir = "~/tasks"      # Datenverzeichnis
editor = "hx"        # optional
theme = "tokyonight" # wird von der TUI geschrieben (`t`)
buckets = ["work"]   # angezeigte Buckets, wird von der TUI geschrieben (`f`)
show_done = true     # Spalte „Erledigt“ anzeigen, wird von der TUI geschrieben (`v`)
```

- Datenverzeichnis: `--dir` > `dir` aus der Config > `~/tasks`
- Schreiben (Theme, Filter) erfolgt über `toml_edit`, Kommentare bleiben erhalten
- Der Bucket-Filter ist bewusst rechnerlokal: Arbeitsrechner zeigt „Arbeit“, privat „Privat“
  – bei denselben synchronisierten Daten. Leer = alle, `"-"` = ohne Bucket

### Board-Konfiguration (`shdo.toml` im Daten-Repo, wird synchronisiert)

```toml
[[status]]
key = "todo"
name = "Offen"

[[status]]
key = "doing"
name = "In Arbeit"

[[status]]
key = "wait"
name = "Wartet"

[[status]]
key = "done"
name = "Erledigt"
```

- `key` steht in den Task-Dateien, `name` ist nur Anzeige und frei änderbar
- Default (s. o.), falls Datei oder Feld fehlt; keys müssen eindeutig sein
- Jeder Status ist eine Spalte im Board. Erster Status = Eingang für neue Tasks, letzter = erledigt
- Begriff: in Daten und Formularen „Status“, „Spalte“ nur fürs Layout (Navigation `h`/`l`)
- Früherer Name `[[columns]]` bzw. `column = …` wird weiter gelesen; beim nächsten
  Statuswechsel schreibt shdo `status`

Optional Buckets (z. B. Privat/Arbeit), gleiche Form wie Status:

```toml
[[buckets]]
key = "home"
name = "Privat"

[[buckets]]
key = "work"
name = "Arbeit"
```

- Ohne `[[buckets]]` ist das Feature unsichtbar (kein Filter, kein Formularfeld)
- key `-` ist reserviert (steht im Filter für „ohne Bucket“)

## Datenmodell

### Verzeichnisstruktur

```
~/tasks/
├── shdo.toml
├── tasks/
│   ├── a3f9.md
│   └── b71c.md
└── archive/
```

`init` legt `.gitkeep` in `tasks/` und `archive/` an, damit Git die Verzeichnisse versioniert.

### Task-Datei (`tasks/<id>.md`)

```markdown
+++
title = "Rechnung bezahlen"
status = "todo"
prio = 1
due = 2026-10-24
created = 2026-10-22T10:41:00+02:00
moved = 2026-10-22T14:03:00+02:00
+++

Kundennr. 4711, bis Freitag

- [x] Rechnung raussuchen
- [ ] überweisen
```

| Feld      | Pflicht | Typ                     | Beschreibung                                        |
|-----------|---------|-------------------------|-----------------------------------------------------|
| `title`   | ja      | String                  | Kurzer Titel                                        |
| `status`  | ja      | String                  | `key` eines Status aus `shdo.toml`                  |
| `created` | ja      | Datum + Zeit (RFC 3339) | Beim Anlegen gesetzt, ändert sich nie               |
| `moved`   | nein    | Datum + Zeit            | Zeitpunkt, seit dem der Task diesen Status hat     |
| `done`    | nein    | Datum + Zeit            | Gesetzt beim Wechsel in den letzten Status, entfernt beim Verlassen |
| `prio`    | nein    | Integer 1–3             | 1 = hoch, 2 = normal (Default), 3 = niedrig         |
| `bucket`  | nein    | String                  | `key` eines Buckets aus `shdo.toml`                 |
| `due`     | nein    | Datum (lokal)           | Fälligkeit ohne Uhrzeit                             |

- Body nach dem Frontmatter: freies Markdown (Notiz). Checkboxen `- [ ]` / `- [x]` werden
  gezählt und auf der Karte als `☐ 1/3` angezeigt – keine echten Subtasks
- Dateiname = ID: 4 Zeichen `[a-z0-9]`, zufällig, eindeutig in `tasks/` und `archive/`;
  wird bei Bedarf länger
- Jede Änderung schreibt ausschließlich die Datei dieses Tasks (atomar über Temp-Datei + Rename)
- Unbekannte Felder und Kommentare im Frontmatter bleiben erhalten, der Body bleibt unverändert
- Tolerant beim Lesen: `prio` außerhalb 1–3 wird geklemmt, Daten dürfen auch als String stehen
- Tasks mit unbekanntem oder fehlendem `status` erscheinen in der ersten Spalte, mit Warnung
  (Datei wird nicht automatisch geändert)
- Tasks mit unbekanntem `bucket` gelten als „ohne Bucket“, mit Warnung
- Ungültige Dateien werden übersprungen und als Warnung gemeldet
- Fehlt `done` in der letzten Spalte (z. B. nach Bearbeitung von Hand), gilt die mtime der Datei

## Bucket-Filter

- Gilt in TUI und `shdo ls`; `done`, `mv`, `rm`, `edit`, `bucket`, `archive` wirken immer auf alle
- Aktiver Filter ist immer sichtbar (TUI-Kopfzeile `⧩ Arbeit · 6 ausgeblendet`, erste Zeile bei `ls`)
- Vererbung: Ist genau ein echter Bucket gewählt, bekommen neue Tasks ihn, sofern kein `@…`
  angegeben ist. Das wird überall sichtbar gemacht: im Formular („aus Filter übernommen“),
  in der Rückmeldung (`@work (aus Filter)`) in TUI und CLI
- Wird ein Task durch Anlegen/Ändern ausgeblendet, sagt die Rückmeldung das
- Bucket auf Karten/in `ls` nur, wenn mehr als ein Bucket sichtbar sein kann

## Sortierung

Offene Spalten:
1. `prio` aufsteigend (ohne `prio` zählt 2)
2. `due` aufsteigend; ohne `due` ans Ende
3. `created` aufsteigend, dann ID als stabiler Tiebreaker

Letzte Spalte: `done` absteigend (zuletzt erledigt oben).

Gilt identisch für TUI und `shdo ls`.

## CLI

```
shdo                              TUI öffnen
shdo <text>                       Neuen Task in der ersten Spalte anlegen
shdo add|a <text>                 dito, explizit
shdo -- <text>                    dito, wenn der Titel mit einem Befehl beginnt
shdo ls [-a] [@bucket]            Board als Text (Filter; -a alle; @bucket einmalig)
shdo bucket <id> <bucket|->       Bucket setzen bzw. entfernen
shdo done <id>                    In die letzte Spalte
shdo mv <id> <status>             Status setzen (key oder Name, Wort-Präfix reicht: „arbeit“)
shdo rm <id>                      Löschen
shdo edit <id>                    Task-Datei im Editor öffnen
shdo sync                         Mit dem Git-Remote synchronisieren
shdo archive                      Tasks der letzten Spalte nach archive/
shdo init                         Struktur anlegen (inkl. git init)
shdo completions <shell>          Completion-Skript ausgeben
Global: --dir <pfad>
```

- IDs werden per eindeutigem Präfix aufgelöst (`shdo edit a3`)
- Tippfehlerschutz: ein einzelnes Wort, das einem Befehl ähnelt (Distanz ≤ 1, z. B. `sycn`),
  legt keinen Task an, sondern zeigt einen Hinweis

### Schnellerfassung (CLI und TUI-Eingabe `n`)

- `shdo Rechnung bezahlen P1 due:fr @home` → Titel „Rechnung bezahlen“, prio 1, due nächster
  Freitag, Bucket home
- `P1`–`P3` (auch klein), `due:…` und `@bucket` an beliebiger Stelle, werden aus dem Titel
  entfernt; Buckets per key, Name oder eindeutigem Präfix (`@arb`), Unbekanntes ist ein Fehler
- Ausgabe: eine Zeile mit ID und Titel

### Datumsformate

- ISO `2026-10-24`, deutsch `24.10.` (nächstes Vorkommen) und `24.10.2026` / `24.10.26`
- Relativ `+3d`, `+2w`, `+1m`
- `heute`, `morgen`, `übermorgen`
- Wochentage `mo`–`so` → nächstes Vorkommen (nicht heute)
- Ungültige Eingaben ergeben eine klare Fehlermeldung

### Shell-Completion

- `shdo completions <shell>` (fish, bash, zsh, elvish, powershell) gibt das Skript aus;
  `mise run install` richtet die Fish-Completion automatisch ein
- Statisch für alle Shells (Befehle, Optionen) über `clap_complete`
- Fish zusätzlich dynamisch über den versteckten Befehl `shdo __complete <art>`:
  - IDs mit Titel und Status als Beschreibung bei `done`/`mv`/`rm`/`edit`/`bucket`
  - Status bei `mv <id> …`, Buckets (und `-`) bei `bucket <id> …`
  - `@bucket` bei `ls` und überall, wo das Wort mit `@` beginnt
  - `due:…`-Kurzformen mit aufgelöstem Datum (`due:fr  fr 23.10.2026`)
  - beachtet `--dir` auf der Kommandozeile; Fehler erzeugen nie Ausgabe
- Nie Dateinamen vorschlagen, außer Verzeichnisse für `--dir`

### `shdo ls`

- Spalten nacheinander mit Name und Anzahl; pro Task: ID, Prio (bzw. ✓), Due/Erledigt-Datum, Titel
- Farbig nur, wenn stdout ein Terminal ist; Überfälliges rot; Warnungen auf stderr

### `shdo sync`

1. `git add -A`
2. `git commit -m "shdo sync <hostname> <zeitstempel>"`, nur wenn es Änderungen gibt
3. Mit Upstream: `git pull --rebase`, `git push`; ohne Upstream: `git push -u <remote> HEAD`
4. Ohne Remote: nur lokal committen und darauf hinweisen

- Bei Konflikten oder Fehlern: abbrechen, git-Ausgabe anzeigen, nichts automatisch auflösen
- Aus der TUI im Hintergrund-Thread und ohne Prompts
  (`GIT_TERMINAL_PROMPT=0`, `ssh -o BatchMode=yes`), Ergebnis in der Statuszeile

## TUI

### Layout

```
 shdo ~/tasks                                       [ 7 offen · 1 überfällig ]
┌Offen─────────────────┐┌In Arbeit─────────────┐┌Erledigt──────────────┐
│→ P1 Rechnung bezahlen││  P2 Angebot schreiben││  ✓  Steuer abgeben   │
│     fr 24.10. · ☐ 1/3││     seit 3 Tagen     ││     gestern          │
└─────────────1 von 3──┘└─────────────────────1┘└─────────────────────2┘
 Statuszeile: Ergebnis der letzten Aktion / Eingabefeld / Warnungen
 [h/l → Spalte] [j/k → Karte] [H/L → verschieben] [n → neu] … [? → Hilfe] [q → Ende]
```

- Kopfzeile: Datenpfad links, Zähler (offen, überfällig) rechts
- Spalten gleich breit; aktive Spalte mit hervorgehobenem Rahmen, Zähler „x von y“ unten rechts
- Passen nicht alle Spalten (min. 24 Zeichen), wird horizontal mitgescrollt (◂ ▸ als Hinweis)
- Karten (2 Zeilen): Marker `→`, Prio bzw. `✓`, Titel (gekürzt); darunter Due, „seit X Tagen“
  (nicht in der ersten Spalte), Checkliste, `≡` bei Notiz. Passt die Zeile nicht, fallen
  zuerst die unwichtigsten Teile weg
- Prio farblich: 1 rot/fett, 2 neutral, 3 gedimmt; überfällig rot, heute fällig hervorgehoben
- Tastenleiste unten permanent sichtbar und kontextabhängig (Normal, Formular, Dialoge);
  bricht auf eine zweite Zeile um, wenn eine nicht reicht
- `?` vollständige Tastenübersicht inkl. Datumsformate und Warnungen
- Lange Spalten scrollen mit der Selektion

### Tasten

| Taste           | Aktion                                                  |
|-----------------|---------------------------------------------------------|
| `h` / `←`       | Spalte links                                            |
| `l` / `→`       | Spalte rechts                                           |
| `j` / `↓`       | Nächste Karte                                           |
| `k` / `↑`       | Vorherige Karte                                         |
| `g` / `G`       | Erste / letzte Karte                                    |
| `H` / `Shift+←` | Karte eine Spalte nach links (Selektion folgt)          |
| `L` / `Shift+→` | Karte eine Spalte nach rechts (Selektion folgt)         |
| `x`             | Erledigt bzw. wieder offen (Selektion bleibt in Spalte) |
| `n`             | Neue Karte: Formular, Status der Spalte vorbelegt       |
| `E`             | Karte im Formular bearbeiten                            |
| `b`             | Bucket setzen (Auswahlliste)                            |
| `f`             | Bucket-Filter (Checkboxen)                              |
| `v`             | Letzte Spalte (Erledigt) ein-/ausblenden, lokal gespeichert; Kopfzeile zeigt „N Erledigt ausgeblendet“ |
| `r`             | Titel umbenennen                                        |
| `1`–`3`         | Prio setzen                                             |
| `D`             | Due setzen (leere Eingabe entfernt)                     |
| `⏎`             | Detailansicht (Felder + Notiz), dort `e` für Editor     |
| `e`             | Task-Datei im Editor öffnen, danach neu laden           |
| `d`             | Löschen (mit Bestätigung `j`)                           |
| `s`             | Sync im Hintergrund                                     |
| `R`             | Neu laden                                               |
| `t`             | Theme wählen (Live-Vorschau, `⏎` speichert, `Esc` verwirft) |
| `?`             | Hilfe                                                   |
| `q` / `Esc`     | Beenden bzw. Eingabe/Dialog abbrechen                   |

- Eingabefelder (`r`, `D`): `←`/`→`, `Home`/`End`, `Backspace`/`Entf`, `Ctrl+U` leert; bei
  ungültiger Eingabe bleibt das Feld offen und die Statuszeile zeigt den Fehler

### Formular (neu / bearbeiten)

```
┌ Neuer Task ─────────────────────────────────────────────────┐
│ ▸ Titel     Folien bauen                                    │
│             P1–P3 · due:fr · @bucket im Titel gehen auch    │
│   Status    ‹ Offen ›                                       │
│   Prio      ‹ P1 hoch ›                                     │
│   Fällig    fr                        → 23.10.2026 (morgen) │
│   Bucket    ‹ Arbeit ›  aus Filter übernommen               │
│   Notiz                                                     │
└─────────────────────────────────────────────────────────────┘
```

- Felder: Titel, Status, Prio, Fällig, Bucket (nur wenn definiert), Notiz (einzeilig)
- `Tab`/`↓` bzw. `⇧Tab`/`↑` wechseln das Feld, `←`/`→`/`Space` (bzw. `h`/`l`) ändern
  Auswahlfelder, `1`–`3` im Prio-Feld, `Enter` speichert aus jedem Feld, `Esc` verwirft
- Schneller Weg bleibt: `n`, `Rechnung P1 due:fr`, `Enter`. Tokens im Titel haben Vorrang;
  das Formular zeigt sie live („erkannt: P1 · morgen“) und an den betroffenen Feldern
  („← Titel: P1“)
- Fälligkeit mit Live-Vorschau des erkannten Datums bzw. Fehlerhinweis
- Bearbeiten: mehrzeilige Notizen sind schreibgeschützt („im Editor bearbeiten (e)“)
- Ungültige Eingaben: Formular bleibt offen, Fehler in der Statuszeile

### Filter-Popup (`f`)

- Checkboxen je Bucket plus „(ohne Bucket)“, mit Anzahl offener Tasks
- `Space`/`x` an/aus, `o` nur dieser, `a` alle; wirkt sofort als Vorschau
- `Enter` speichert in `config.toml`, `Esc` stellt den vorherigen Filter wieder her
- Alles oder nichts gewählt = alle
- Änderungen werden sofort gespeichert
- Externe Änderungen (z. B. `shdo …` in einem anderen Pane) werden per Fingerprint
  (Dateien, mtime, Größe) alle 500 ms erkannt und neu geladen
- Beim Öffnen des Editors wird die TUI sauber verlassen und danach wiederhergestellt;
  das Terminal wird auch bei Panics zurückgesetzt

### Themes

- Semantische Farb-Tokens: `bg`, `fg`, `dim`, `border`, `border_active`, `title`, `accent`,
  `selection`, `prio_high`, `prio_low`, `overdue`, `today`, `done`, `key`, `warn`
- Farben als `#rrggbb` oder ANSI-Namen (`red`, `darkgray`, `reset`, …)
- Eingebaut (`themes/*.toml`, per `include_str!` im Binary): `tokyonight` (Default), `nord`, `terminal`
  (ANSI-Farben und Hintergrund des Terminals)
- Eigene Themes: `~/.config/shdo/themes/<id>.toml`, gleiches Format; fehlende Tokens kommen
  vom Default-Theme; gleiche ID überschreibt ein eingebautes; Fehler werden Warnungen

```toml
name = "Mein Theme"
[colors]
bg = "#1a1b26"
border_active = "#9ece6a"
```

## Code-Struktur

```
src/
├── main.rs        # CLI (clap), Befehle, Tippfehlerschutz, Editor-Aufruf
├── config.rs      # App-Config (config.toml)
├── store.rs       # Datenverzeichnis, shdo.toml, Laden, IDs, Präfixsuche, Archiv
├── task.rs        # Task-Modell, Frontmatter lesen/schreiben
├── board.rs       # Gruppierung nach Spalten, Sortierung, Verschieben
├── dates.rs       # Datumsparser und deutsche Kurzformen
├── quickadd.rs    # Schnellerfassungs-Syntax
├── git.rs         # sync
├── theme.rs       # Themes laden
└── tui/
    ├── mod.rs     # Event-Loop, Terminal-Setup, Editor-Suspend
    ├── app.rs     # App-State, Aktionen, Tastenverarbeitung
    └── ui.rs      # Rendering
themes/            # eingebaute Themes
```

## Entwicklung

mise-Tasks in `.mise-tasks/`:

| Task           | Zweck                                                  |
|----------------|--------------------------------------------------------|
| `demo`         | TUI mit Beispieldaten in `target/demo` (`--reset`)     |
| `test`         | `cargo test`                                           |
| `fmt`          | `cargo fmt`                                            |
| `check:fmt`    | Formatierung prüfen                                    |
| `check:clippy` | Clippy mit `-D warnings`                               |
| `check:all`    | fmt + clippy + Tests                                   |
| `install`      | `cargo install --path .` + Fish-Completion             |

Tests arbeiten in temporären Verzeichnissen, nie in `~/tasks`; Datumstests mit fester Referenzzeit.

## Später / Ideen

- Kalender-Seitenleiste (Monat mit KW, Wochenenden soft, Feiertage BW deutlich, Tage mit
  Fälligkeiten markiert, „Demnächst“-Liste; `Tab` fokussiert, Filter auf Tag/KW; Feiertage
  als Tabelle je Bundesland, später `holidays = "by"` konfigurierbar) – Details offen

- Auto-Sync beim Start/Beenden der TUI (optional, per Config)
- Weitere eingebaute Themes (Catppuccin Mocha/Latte, Gruvbox)
- Eigene Themes in der TUI anlegen/löschen – löschbar nur selbst angelegte, nie eingebaute
- Suche/Filter mit `/`
- Zurückstellen (`wait = <datum>`: erst ab Datum anzeigen)
- WIP-Limits je Spalte (optional in `shdo.toml`)
- README mit Installation, Einrichtung von Repo und Remote, Tastenübersicht
