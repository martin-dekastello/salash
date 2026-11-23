# README demo. Record with demo/record.sh, which copies demo/ to /tmp/salash-demo first.
Output docs/demo.gif

Require claude
Set Shell "bash"
Set FontSize 15
Set Width 1400
Set Height 760
Set Padding 16
Set Theme "Catppuccin Mocha"
Set TypingSpeed 45ms
Set WindowBar Colorful

Hide
Type "export SALASH_CONFIG=/tmp/salash-demo/config.toml PATH=$PWD/target/release:$PATH PS1='$ '; cd /tmp/salash-demo/work; clear"
Enter
Show

Type "salash"
Sleep 400ms
Enter
Wait+Screen /quit/
Sleep 1.5s

# To-do: pick the failing-test issue and start Claude on it.
Type "0"
Sleep 1s
Down 2
Sleep 600ms
Up 2
Sleep 800ms
Type "c"
# A new folder makes Claude ask "do you trust this folder?". Answer it off camera. Down +
# Enter picks "Yes" there, and does nothing on Claude's empty prompt once it's trusted.
Wait+Screen@40s /trust this folder|accept edits/
Hide
Sleep 500ms
Down
Enter
Wait+Screen@40s /accept edits/
Show
Sleep 1s
Type "Make test_partial_tiles_count pass. Only edit tiles.py, don't run anything."
Sleep 500ms
Enter
Sleep 2.5s

# A second tab on the same issue: a shell in the same worktree, while Claude works.
Ctrl+a
Type "t"
Sleep 1.5s
Type "git branch --show-current"
Enter
Sleep 2s

# Claude finished (● on the issue and its tab): run the tests next to it, then see its diff.
Wait+Screen@180s /● TS-47/
Sleep 1s
Type "python3 test_tiles.py"
Enter
Sleep 2.5s
Ctrl+a
Type "1"
Sleep 4s

# Done: the note moves and the session is logged.
Ctrl+a
Type "s"
Sleep 600ms
Type "d"
Sleep 1.5s
Type "2"
Sleep 3s

Hide
Type "q"
Sleep 500ms
Type "y"
Sleep 1s
