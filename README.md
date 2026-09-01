# Ghostterm

A macOS terminal emulator built in Rust with [GPUI](https://github.com/zed-industries/zed) and [libghostty-vt](https://github.com/uzaaft/libghostty-rs). Each window has a left sidebar of sessions (tabs) and a terminal that talks to your login shell over a PTY.

## Features

- Tabbed sessions in a left sidebar: add with **⌘T** or the **+** button, close with **⌘W** or **×**
- Mouse selection: click-drag, double-click a word, triple-click a line, Option-drag for a block
- Line and word movement: **⌘← / ⌘→** jump to the start or end of the line; **⌥← / ⌥→** move by word
- **⌘-click** `http(s)` URLs to open them in the browser (hold **⌘** to highlight the link first)
- **⌘-click** filesystem paths to open Finder at that folder (a file path opens the parent directory)
- Typing `exit` in the top-level shell closes that tab; the last tab closes the window without quitting the app

## Requirements

- macOS
- [Rust](https://rustup.rs/) (stable)
- [Xcode](https://developer.apple.com/xcode/) (not only Command Line Tools) so GPUI can compile Metal shaders
- [Zig](https://ziglang.org/) for the libghostty-vt build

The repo’s `.cargo/config.toml` sets `DEVELOPER_DIR` to `/Applications/Xcode.app/Contents/Developer` when it is not already set.

## Run

```bash
cargo run
```

The window title is **Ghostterm**. Your `$SHELL` is used when present (otherwise `/bin/zsh`).

## Shortcuts

| Action | Shortcut |
| --- | --- |
| New tab | ⌘T |
| Close tab | ⌘W |
| Quit | ⌘Q |
| Start / end of line | ⌘← / ⌘→ |
| Previous / next word | ⌥← / ⌥→ |
| Open URL or folder | ⌘-click |

## Roadmap

### Daily driver

- [ ] Copy and paste (⌘C / ⌘V)
- [ ] Delete by word and line (⌥⌫ / ⌘⌫, maybe ⌥⌦)
- [ ] IME / composed input (accents, CJK, dead keys)
- [ ] Bell (audible or visual) and richer tab titles from the process
- [ ] Clear screen (⌘K)

### Selection and clipboard

- [ ] Copy the current selection with ⌘C
- [ ] Right-click menu: Copy, Paste, Open Link
- [ ] Select all in scrollback (⌘A)
- [ ] Optional copy-on-select

### Window and sessions

- [ ] New window (⌘N), including reopen from the Dock when no windows are open
- [ ] Remember window position and size across launches
- [ ] Draggable split to resize the tab sidebar vs the terminal
- [ ] Drag to reorder tabs in the sidebar
- [ ] Rename tabs
- [ ] Option to keep a tab open after the shell exits

### Rendering

- [ ] Fix Ghostty theme functionality so palette, foreground, background, and cursor colors actually apply
- [ ] Italic, underline, strikethrough, dim, and fuller 256/truecolor support
- [ ] Wide glyphs and emoji so the grid and cursor stay aligned
- [ ] Image protocols (iTerm2 / Kitty) once text rendering is solid

### Config

- [ ] Settings UI and config file owned by Ghostterm (not libghostty): a basic menu or window plus a user-editable file for common options as we add them
- [ ] Font family and size, colors, and scrollback length
- [ ] Config file for theme
- [ ] Input and click behavior (including ⌘-click file vs folder)

### Later

- [ ] Find in scrollback (⌘F)
- [ ] Split panes
- [ ] OSC 7 working directory / remote-aware paths
- [ ] Underline URLs and paths without holding ⌘
- [ ] Tests for key encoding, URL/path detection, and shell-exit

## License

MIT. See [LICENSE](LICENSE).
