# VHS tape for heisensim README hero recording
# https://github.com/charmbracelet/vhs
#
# Prerequisites: heisensim on PATH, vhs installed
# Record: vhs demo.tape

Output demo.gif
Output demo.webm

Set Shell "bash"
Set FontSize 14
Set Width 960
Set Height 600
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

# Clean workspace + PATH setup
Hide
Type "cd $(mktemp -d) && export PATH=$HOME/.local/bin:$PATH && export PS1='$ '"
Enter
Sleep 500ms
Show

# --- Scene 1: Init ---
Type "# Generate a chaos testing config in one command"
Enter
Sleep 800ms
Type "heisensim init --preset microservice"
Enter
Sleep 2500ms

# --- Scene 2: Simulate a single seed ---
Type "# Simulate 5 minutes of chaos — deterministic, no cluster"
Enter
Sleep 800ms
Type "heisensim simulate --seed 0x42 --duration 5m --config heisensim.toml"
Enter
Sleep 4500ms

# --- Scene 3: Explore 20 seeds ---
Type "# Explore 20 seeds at once — find every failure mode"
Enter
Sleep 800ms
Type "heisensim explore --simulate --seeds 20 --duration 5m --config heisensim.toml"
Enter
Sleep 3500ms

# --- Scene 4: Diff two seeds ---
Type "# Compare two seeds side-by-side"
Enter
Sleep 800ms
Type "heisensim diff --seed-a 0x01 --seed-b 0x02 --duration 5m --config heisensim.toml"
Enter
Sleep 4s

# Hold final frame
Sleep 2s
