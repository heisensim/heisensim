# heisensim demo recording
# Generates demo.gif for the landing page and README

Output demo.gif

Set Shell "zsh"
Set FontSize 15
Set Width 960
Set Height 540
Set Theme "Catppuccin Mocha"
Set Padding 20
Set TypingSpeed 30ms

# Work from a disposable temp dir to avoid stray files
Type "cd $(mktemp -d) && clear"
Enter
Sleep 500ms

# Step 1: Show version
Type "heisensim --version"
Enter
Sleep 1s

# Step 2: Run with mock mode — short and punchy
Type "# Chaos test — no cluster needed"
Enter
Sleep 300ms
Type "heisensim run --mock --seed 42 --duration 30s --warmup 5s --faults network-delay,network-loss"
Enter
Sleep 4s

# Step 3: Explore with coverage
Type "# Find bugs faster with coverage-guided exploration"
Enter
Sleep 300ms
Type "heisensim explore --mock --seeds 3 --explore-strategy coverage"
Enter
Sleep 6s

# Final pause
Sleep 2s
