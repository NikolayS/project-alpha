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
Sleep 3s
Type "clear"
Enter
Sleep 500ms
Show

# --- Connect: pause so the viewer can read the typed command ---
Type "rpg"
Sleep 1500ms
Enter
Sleep 3s

# --- Launch /top: pause before Enter, then watch the live TUI ---
# Visible smart features appearing here (no overlay text in vhs):
#   • header connection LED ●  (green = fresh sample, yellow = stale)
#   • "@ HH:MM:SS UTC" clock format (not a raw unix epoch)
#   • per-state coloring (idle-in-tx = yellow, active = bright green)
#   • per-wait coloring (Lock = red, IO = blue, LWLock = light yellow)
#   • qtime warn/crit thresholds (>1 s yellow, >30 s red)
#   • app/client/backend columns auto-shown ≥120 cols (responsive layout)
Type "/top"
Sleep 1500ms
Enter
Sleep 6s

# --- Cursor navigation: sticks across row-count changes between ticks ---
Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 700ms
Down
Sleep 1500ms

# --- Pause to let the viewer watch counts/waits change live ---
Sleep 4s

# --- Move back up; cursor clamps even if rows shrink mid-tick ---
Up
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 500ms
Up
Sleep 1500ms

# --- One more live refresh to show stable behaviour ---
Sleep 3s

# --- Quit /top with q (single-key, no Enter needed in TUI) ---
Type "q"
Sleep 2s

# --- Headless `--once` mode: scriptable / CI snapshot ---
# Same renderer as the TUI, written as plain text to stdout.
Type "/top --once"
Sleep 1500ms
Enter
Sleep 5s

# --- Clean exit from rpg ---
Type "\q"
Sleep 1500ms
Enter
Sleep 1s

# --- Hidden cleanup: kill workload group ---
Hide
Type "kill %1 2>/dev/null; wait 2>/dev/null; pkill -f top-workload 2>/dev/null; true"
Enter
Sleep 800ms
