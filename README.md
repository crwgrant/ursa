# Ghostterm

A macOS terminal emulator built in Rust with [GPUI](https://github.com/zed-industries/zed) and [libghostty-vt](https://github.com/uzaaft/libghostty-rs). Each window has a left sidebar of sessions (tabs) and a terminal that talks to your login shell over a PTY.

## Features

- Tabbed sessions in a left sidebar: add with **⌘T** or the **+** button, close with **⌘W** or **×**
- Mouse selection: click-drag, double-click a word, triple-click a line, Option-drag for a block
- Line and word movement: **⌘← / ⌘→** jump to the start or end of the line; **⌥← / ⌥→** move by word
- Delete by word and line: **⌥⌫** / **⌥⌦** kill the previous or next word; **⌘⌫** / **⌘⌦** kill to the start or end of the line
- **⌘-click** `http(s)` URLs to open them in the browser (hold **⌘** to highlight the link first)
- **⌘-click** filesystem paths to open Finder at that folder (a file path opens the parent directory)
- Right-click a URL, URI, or path for **Copy**, **Paste**, and **Open Link** (or **Open Folder**)
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
| Delete previous / next word | ⌥⌫ / ⌥⌦ |
| Delete to start / end of line | ⌘⌫ / ⌘⌦ |
| Open URL or folder | ⌘-click or right-click → Open Link |
| Copy link | Right-click a URL/path → Copy |
| Copy selection | ⌘C |
| Paste | ⌘V or right-click a URL/path → Paste |

## Roadmap

### Daily driver

- [x] Copy and paste (⌘C / ⌘V)
- [x] Delete by word and line (⌥⌫ / ⌘⌫ / ⌥⌦ / ⌘⌦)
- [ ] IME / composed input (accents, CJK, dead keys)
- [ ] Bell (audible or visual) and richer tab titles from the process
- [ ] Clear screen (⌘K)

### Selection and clipboard

- [x] Copy the current selection with ⌘C
- [x] Right-click menu: Copy, Paste, Open Link
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
- [ ] Create a test suite (see [Test suite](#test-suite))

## Test suite

Not implemented yet. When we add one, start with unit tests around pure logic, then a few integration checks that do not need to launch the full GPUI window.

### Input

- Shift-produced characters (`:@#!` and the rest of the punctuation set) encode to the PTY
- ⌘← / ⌘→ send beginning/end of line; ⌥← / ⌥→ send word jumps
- ⌥⌫ / ⌥⌦ send word-kill; ⌘⌫ / ⌘⌦ send kill to start/end of line
- Reserved shortcuts (⌘Q / ⌘T / ⌘W) are not forwarded as terminal keys

### Links and paths

- `http://`, `https://`, and `www.` matches, including wrapped lines and trailing punctuation
- OSC 8 hyperlinks vs autodetection
- Absolute, `~/`, and `file://` paths; files resolve to the parent directory; missing paths are ignored
- Hover ranges cover the full URL or path

### Sessions and window

- Top-level shell exit closes that tab
- Last tab closes the window without quitting the app
- Nested `exit` (subshell) does not close the tab
- Adding and closing tabs updates the active index

### Terminal surface

- Selection gestures (drag, double-click word, triple-click line, Option block)
- Copy/paste once clipboard is wired
- Scrollback bounds and clear-screen (⌘K)
- Theme: palette, foreground, background, and cursor actually change

### Settings

- Config file parse/round-trip for the Ghostterm-owned settings (not libghostty)
- Invalid or missing files fall back to defaults
- Window position, size, and sidebar split restore

## License

MIT. See [LICENSE](LICENSE).
