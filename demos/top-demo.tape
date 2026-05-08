Output demos/top-demo.gif
Set FontSize 15
Set Width 1400
Set Height 800
Set Theme "Dracula"
Set Shell "zsh"
Set TypingSpeed 30ms

# --- Hidden setup: PATH + connection env + start workload + clear ---
Hide
Type "export PATH=/Users/nik/github/rpg/target/release:$PATH"
Enter
Sleep 200ms
Type "export PGHOST=localhost PGPORT=55433 PGUSER=postgres PGDATABASE=postgres"
Enter
Sleep 200ms
Type "bash /Users/nik/github/rpg/demos/top-workload.sh &"
Enter
Sleep 2500ms
Type "clear"
Enter
Sleep 500ms
Show

# --- Connect ---
Type "rpg"
Sleep 400ms
Enter
Sleep 2500ms

# --- Launch /top ---
Type "/top"
Enter
Sleep 5s

# --- Move cursor down through rows ---
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 1500ms

# --- Watch live refresh: counts and waits change ---
Sleep 4s

# --- Move back up to the top ---
Up
Sleep 400ms
Up
Sleep 400ms
Up
Sleep 400ms
Up
Sleep 400ms
Up
Sleep 1500ms

# --- One more live refresh to show stable behaviour ---
Sleep 3s

# --- Quit /top ---
Type "q"
Sleep 1500ms

# --- Demonstrate the headless `--once` mode ---
Type "/top --once"
Enter
Sleep 4s

# --- Clean exit from rpg ---
Type "\q"
Enter
Sleep 1s

# --- Hidden cleanup: kill workload group ---
Hide
Type "kill %1 2>/dev/null; wait 2>/dev/null; pkill -f top-workload 2>/dev/null; true"
Enter
Sleep 800ms
