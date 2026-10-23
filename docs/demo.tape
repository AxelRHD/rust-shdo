# Records docs/demo.gif – run via `mise run docs:gif`, which prepares the sample data
# and the environment (SHDO_CONFIG_DIR, a `shdo` wrapper with --dir on PATH).

Output docs/demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1400
Set Height 660
Set Padding 16
Set Theme "nord"
Set TypingSpeed 50ms

Hide
Type "export PS1='$ ' HOME=$SHDO_GIF_HOME PATH=$SHDO_GIF_BIN:$PATH"
Enter
Type "clear"
Enter
Show

# Quick capture from the shell
Type "shdo Order cables P1 due:mon @work +hardware"
Sleep 400ms
Enter
Sleep 1.5s

# Open the board and move a card on
Type "shdo"
Enter
Sleep 2.5s
Type "j"
Sleep 500ms
Type "L"
Sleep 1.5s
Type "h"
Sleep 600ms

# New task in the form: tokens in the title are recognized live
Type "n"
Sleep 700ms
Type "Plan sprint due:mon @work +dev"
Sleep 1.5s
Enter
Sleep 1.2s

# Filter: buckets, then the tags tab
Type "f"
Sleep 1s
Right
Sleep 1s
Type "o"
Sleep 1.2s
Enter
Sleep 2s
Type "f"
Sleep 500ms
Right
Sleep 400ms
Type "a"
Sleep 600ms
Enter
Sleep 900ms

# View toggles: calendar and done column
Type "c"
Sleep 1.2s
Type "c"
Sleep 900ms
Type "v"
Sleep 1.2s
Type "v"
Sleep 900ms

# Settings page: how long done tasks stay visible
Type ","
Sleep 1.2s
Down
Sleep 400ms
Down
Sleep 600ms
Right
Sleep 1.5s
Left
Sleep 900ms
Escape
Sleep 700ms

# Theme picker with live preview
Type "t"
Sleep 600ms
Type "j"
Sleep 800ms
Type "j"
Sleep 800ms
Type "j"
Sleep 800ms
Escape
Sleep 600ms

Type "q"
Sleep 500ms
