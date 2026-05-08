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

# --- Connect with global --show-keys (turns on the on-screen
# --- "key pressed" overlay for every TUI command in this rpg session). ---
Type "rpg --show-keys"
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
# --- header is visible while body content scrolls underneath. The corner
# --- key overlay (⌨) labels each press as it happens. ---
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 600ms
Down
Sleep 800ms
PageDown
Sleep 1800ms
PageDown
Sleep 2s
PageUp
Sleep 1800ms

# --- Cycle sort columns with → / ← (or </>); reverse with r. The active
# --- column header carries ▼ (desc) / ▲ (asc). Arrow follows each press. ---
Right
Sleep 2500ms
Right
Sleep 2500ms
Right
Sleep 2500ms
Type "r"
Sleep 2500ms
Left
Sleep 2500ms
Left
Sleep 2500ms

# --- Toggle extended columns: app, client, backend appear ---
Type "e"
Sleep 3500ms

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
Sleep 1500ms
Enter
Sleep 4s

# --- Pick a row and ask to cancel it. The footer flips to a CANCEL
# --- confirmation showing pid / user@db / state / qtime / query summary;
# --- y fires, n cancels. We press n to keep the demo idempotent. ---
Down
Sleep 800ms
Down
Sleep 800ms
Type "k"
Sleep 3500ms
Type "n"
Sleep 1500ms

# --- Same flow with Shift-K (TERMINATE — heavier weapon, separate verb
# --- in the footer). Press n again to dismiss without firing. ---
Type "K"
Sleep 3500ms
Type "n"
Sleep 1500ms

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
