Output demos/top-demo.gif
Set FontSize 14
Set Width 1400
Set Height 800
Set Theme "Dracula"
Set Shell "zsh"
Set TypingSpeed 40ms
Set PlaybackSpeed 1.0

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
Sleep 4s
Type "clear"
Enter
Sleep 500ms
Show

# --- Connect ---
Type "rpg"
Sleep 1500ms
Enter
Sleep 3s

# --- Launch /top with default 1 s refresh ---
# Visible features (all default):
#   • two-line header: db/user/pg/recovery/uptime/clock UTC
#                      + active/idle-in-tx/wait/total/max + longest-tx,
#                        longest-q + cumulative deadlocks + temp_files + ●
#   • activity table — pid, user, db, state, wait, qtime, xtime, locks, query
#   • wait coloring follows the pg_ash palette (CPU green, IO vivid blue,
#     Lock red, LWLock pink, IPC cyan, Client yellow, Timeout orange,
#     IdleTx light yellow, …) and uses ':' as the Type:Event delimiter
#   • qtime warn (>1 s yellow) / crit (>30 s red)
#   • sticky table header — rows scroll, header stays
Type "/top"
Sleep 1500ms
Enter
Sleep 6s

# --- Cursor navigation: scroll past the visible window so the sticky
# --- header is visible while body content scrolls underneath. ---
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 500ms
Down
Sleep 800ms
PageDown
Sleep 1500ms
PageDown
Sleep 2s
PageUp
Sleep 1500ms

# --- Toggle extended columns: app, client, backend appear ---
Type "e"
Sleep 4s

# --- Collapse back to default columns ---
Type "e"
Sleep 2s

# --- Set refresh delay to 0.5 s via the interactive prompt ---
Type "s"
Sleep 1500ms
Backspace
Backspace
Backspace
Backspace
Type "0.5"
Sleep 1200ms
Enter
Sleep 5s

# --- Quit /top with q ---
Type "q"
Sleep 1500ms

# --- Headless `--once` mode for scripting / CI snapshots ---
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
