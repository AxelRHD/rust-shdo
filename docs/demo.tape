# Records docs/demo.gif – run via `mise run docs:gif`, which prepares the sample data
# and the environment (SHDO_CONFIG_DIR, --dir via the `shdo` wrapper on PATH).

Output docs/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1240
Set Height 620
Set Padding 16
Set Theme "nord"
Set TypingSpeed 55ms

Hide
Type "export PS1='$ ' HOME=$SHDO_GIF_HOME"
Enter
Type "clear"
Enter
Show

# Quick capture from the shell
Type "shdo Book flights P1 due:fri @home"
Sleep 400ms
Enter
Sleep 1.5s

# Open the board
Type "shdo"
Enter
Sleep 2.5s

# Move through the cards and start working on one
Type "j"
Sleep 500ms
Type "j"
Sleep 600ms
Type "L"
Sleep 1.5s
Type "h"
Sleep 800ms

# New task in the form, with the date recognized live
Type "n"
Sleep 800ms
Type "Plan sprint due:mon"
Sleep 1.2s
Tab
Sleep 400ms
Tab
Sleep 400ms
Left
Sleep 800ms
Enter
Sleep 1.5s

# Bucket filter: only work
Type "f"
Sleep 1s
Type "j"
Sleep 400ms
Type "o"
Sleep 1.5s
Escape
Sleep 800ms

# Theme picker with live preview
Type "t"
Sleep 800ms
Type "j"
Sleep 900ms
Type "j"
Sleep 900ms
Type "j"
Sleep 900ms
Escape
Sleep 800ms

Type "q"
Sleep 500ms
