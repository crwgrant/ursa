# Ghostterm

A terminal emulator built in Rust with [GPUI](https://github.com/zed-industries/zed) and [libghostty-vt](https://github.com/uzaaft/libghostty-rs). Each window can show a left sidebar of sessions; each session has horizontal tabs, and each tab can split into panes that each talk to your login shell over a PTY. **Settings → Session sidebar** can hide the list so the window is only horizontal tabs.

## Features

- Sessions in a left sidebar: add with **⌘⇧T** or the **+** button, close with **⌘⇧W** or **×**, switch with **⌃1–⌃9**, drag to reorder; drag the right edge to resize, double-click to reset
- Horizontal tabs per session: add with **⌘T** or the tab-bar **+**, close a tab’s last pane with **⌘W** or the tab **×**, switch with **⌘1–⌘9**
- Split panes inside a tab: **⌘D** splits right, **⌘⇧D** splits down, **⌘] / ⌘[** move focus, click a pane to focus it; drag the gutter to resize, double-click to reset. **⌘W** closes the focused pane; the last pane closes the tab. A new pane starts a **new local login shell** in the current pane’s local working directory. After SSH, that is the directory you ran `ssh` from, not the remote path, and not a second SSH session. Up to 16 panes per tab.
- Mouse selection: click-drag, double-click a word, triple-click a line, Option-drag for a block
- Line and word movement: **⌘← / ⌘→** jump to the start or end of the line; **⌥← / ⌥→** move by word
- Delete by word and line: **⌥⌫** / **⌥⌦** kill the previous or next word; **⌘⌫** / **⌘⌦** kill to the start or end of the line
- **⌘-click** `http(s)` URLs to open them in the browser (hold **⌘** to highlight the link first)
- **⌘-click** local filesystem paths to open Finder at that folder (a file path opens the parent directory). Paths from a remote OSC 7 session are not treated as folders on this machine
- Right-click a URL, URI, or path for **Copy**, **Paste**, and **Open Link** (or **Open Folder**)
- In-app toasts for copy and paste feedback (bottom-right, click or wait to dismiss)
- **⌘K** clears the screen and scrollback; the shell redraws the prompt at the top
- Settings window (**⌘,** or **Ghostterm → Settings**) for theme, font family, size, cursor style, scrollback, session restore, and the session sidebar; values are stored in a Ghostterm-owned config file you can also edit by hand
- **Settings → Session sidebar → Off** hides the sessions list and session shortcuts; only horizontal tabs remain. **Keep sessions** still restores that one session’s tabs and panes
- **Settings → Sessions → Keep sessions** restores sessions, tabs, split panes, working directories, and on-screen text on reopen, and keeps a tab open if its shell exits. **Close sessions** starts fresh and does not save previous sessions
- New window with **⌘N** or **File → New Window**; closing the last session leaves the app running so a Dock click (or ⌘N) can open another
- Typing `exit` in the top-level shell closes that pane unless **Keep sessions** is on. The last pane in a tab closes the tab; the last tab in a session closes the session; the last session closes the window without quitting the app

## Requirements

- macOS or Windows
- [Rust](https://rustup.rs/) (stable)
- [Zig](https://ziglang.org/) for the libghostty-vt build
- On macOS, [Xcode](https://developer.apple.com/xcode/) (not only Command Line Tools) so GPUI can compile Metal shaders

The repo’s `.cargo/config.toml` sets `DEVELOPER_DIR` to `/Applications/Xcode.app/Contents/Developer` when it is not already set.

## Run

```bash
cargo run
```

The window title is **Ghostterm**. On macOS, `$SHELL` is used when present (otherwise `/bin/zsh`). On Windows, a Unix-style `$SHELL` such as `/bin/zsh` is ignored. If [Git for Windows](https://gitforwindows.org/) is installed, sessions start Git Bash (`Git\bin\bash.exe`); otherwise Windows PowerShell, or `%COMSPEC%` if PowerShell is missing. A Windows `$SHELL` pointing at an `.exe` still wins when set.

## Shortcuts

| Action | Shortcut |
| --- | --- |
| New window | ⌘N |
| New session | ⌘⇧T |
| New tab | ⌘T |
| Split right | ⌘D |
| Split down | ⌘⇧D |
| Focus next / previous pane | ⌘] / ⌘[ |
| Close pane (last pane closes the tab) | ⌘W |
| Close session | ⌘⇧W |
| Switch tab | ⌘1–⌘9 |
| Switch session | ⌃1–⌃9 |
| Quit | ⌘Q |
| Start / end of line | ⌘← / ⌘→ |
| Previous / next word | ⌥← / ⌥→ |
| Delete previous / next word | ⌥⌫ / ⌥⌦ |
| Delete to start / end of line | ⌘⌫ / ⌘⌦ |
| Open URL or folder | ⌘-click or right-click → Open Link |
| Copy link | Right-click a URL/path → Copy |
| Copy selection | ⌘C |
| Paste | ⌘V or right-click a URL/path → Paste |
| Clear screen | ⌘K |
| Settings | ⌘, |

On Windows, use **Ctrl+Shift** in place of **⌘** (for example **Ctrl+Shift+T** for a new tab, **Ctrl+Alt+T** for a new session, **Ctrl+Alt+W** to close a session, **Ctrl+Shift+1–9** to switch tabs, **Ctrl+1–9** to switch sessions, **Ctrl+Shift+C** / **Ctrl+Shift+V** to copy and paste, **Ctrl+Shift+D** to split right, **Ctrl+Alt+D** to split down, **Ctrl+Shift+] / Ctrl+Shift+[** to move pane focus). **Ctrl+C** still goes to the shell. Settings is **Ctrl+,**.

## Configuration

Ghostterm reads a TOML file it owns (not libghostty). The file is created the first time you save from **Settings** or click **Open File**.

| Platform | Path |
| --- | --- |
| All | `~/.config/ghostterm/config.toml` |

If `XDG_CONFIG_HOME` is set, that directory is used instead of `~/.config`. An existing folder at the previous locations (`~/Library/Application Support/Ghostterm` on macOS, `%APPDATA%\Ghostterm` on Windows) is still used until you move it.

Window position, size, which monitor it was on, the sidebar split width, and the session/tab/pane layout are stored separately in `window.toml` in the same folder, so moving or resizing the app does not rewrite `config.toml`. Extra windows opened with ⌘N are offset from that saved frame and start with one session. If that monitor is unplugged, the window is centered on the current screen. Drag the border between the sessions list and the terminal to resize; double-click it or use **Reset to Defaults** in Settings to restore the 220px width. With **Keep sessions**, reopening restores the last sessions, tabs, and split trees, each pane’s **local** working directory, and the text that was on screen. A new shell starts in that directory (running commands are not resumed). Remote OSC 7 paths are not restored as a local `cd` and SSH sessions are not restarted. Screen snapshots live in `state/` next to `window.toml` (`s{session}-t{tab}-p{pane}.snp`). **Close sessions** (the default) and quitting with only an unused tab both start clean on the next launch.

```toml
[font]
family = "Menlo"
size = 13

[appearance]
theme = "nord"
themes = "themes"

[terminal]
scrollback_lines = 2000
cursor = "bar" # or "block"
on_exit = "close" # or "keep" — close sessions on quit, or keep and restore them
sessions = true # or false — hide the session sidebar and use horizontal tabs only
```

`theme` is the filename stem of a Ghostty `.conf` in the themes folder (`nord`, `one-dark`, `tokyo-night`, `catppuccin-mocha`, `catppuccin-frappe`, `gruvbox-dark`, `solarized-light`). `themes` is that folder: relative to `config.toml`, or absolute.

The default `themes/` directory is written next to `config.toml` on first launch. Each file is a Ghostty theme (the format from [ghostty.org](https://ghostty.org/docs/features/theme)): `background`, `foreground`, `cursor-color`, and `palette = 0`–`15`. Ghostterm derives sidebar and tab colors from those values.

Optional Ghostterm keys (Ghostty ignores them): `text` (active tab titles) and `text-dim` (SESSIONS, inactive tabs, Settings). If `text-dim` is omitted, it is mixed from `foreground` and `background`. Other unknown keys are ignored, so files from [catppuccin/ghostty](https://github.com/catppuccin/ghostty) or iterm2-color-schemes can be copied in as-is.

The Settings window writes `config.toml` when you change a value. Editing the config or a file in `themes/` reloads within a couple of seconds (or immediately via **Reload**). Invalid TOML keeps the last good settings and shows a toast; a missing file uses the platform defaults. Unknown keys are ignored so older Ghostterm versions stay compatible.

## Roadmap

### Daily driver

- [x] Copy and paste (⌘C / ⌘V)
- [x] Delete by word and line (⌥⌫ / ⌘⌫ / ⌥⌦ / ⌘⌦)
- [x] Bell (audible or visual) and richer tab titles from the process
- [x] Clear screen (⌘K)

### Selection and clipboard

- [x] Copy the current selection with ⌘C
- [x] Right-click menu: Copy, Paste, Open Link
- [ ] Select all in scrollback (⌘A)
- [ ] Optional copy-on-select

### Window and sessions

- [x] New window (⌘N), including reopen from the Dock when no windows are open
- [x] Horizontal tabs for each session
- [x] Remember window position and size across launches
- [x] Restore sessions, tabs, working directories, and on-screen history
- [x] Draggable split to resize the tab sidebar vs the terminal
- [x] Drag to reorder tabs in the sidebar
- [x] Split panes inside a tab (right/down, focus next/prev, persist the tree)
- [ ] Rename tabs
- [x] Option to keep a tab open after the shell exits
- [x] Option to turn off sessions and have simple terminal with horizontal tabs

### Rendering

- [x] Fix Ghostty theme functionality so palette, foreground, background, and cursor colors actually apply
- [ ] Italic, underline, strikethrough, dim, and fuller 256/truecolor support
- [ ] Wide glyphs and emoji so the grid and cursor stay aligned
- [ ] Image protocols (iTerm2 / Kitty) once text rendering is solid

### Config

- [x] Settings UI and config file owned by Ghostterm (not libghostty): a basic menu or window plus a user-editable file for common options as we add them
- [x] Font family, size, cursor style, scrollback length, and app theme
- [ ] Input and click behavior (including ⌘-click file vs folder)

### Later

- [ ] IME / composed input (accents, CJK, dead keys)
- [ ] Find in scrollback (⌘F)
- [x] Equalize split sizes
- [x] Restore the local working directory of each tab (OSC 7 when the shell sends it, otherwise the process cwd)
- [x] OSC 7 remote-aware paths (keep the hostname; never `cd` or open Finder as if a remote path were local)
- [ ] Zoom the focused pane
- [ ] Rotate a split
- [ ] Tab bar per pane
- [ ] Underline URLs and paths without holding ⌘
- [ ] Create a test suite (see [Test suite](#test-suite))

### Remote / SSH

A pane is always a local PTY and login shell. SSH is a process inside that shell, so split / new tab / restore start another local shell. They do not clone the SSH connection.

- [ ] Show `user@host` (or the OSC 7 host) in the tab title while the pane is remote
- [ ] Detect a remote session without OSC 7 (process tree, `SSH_CONNECTION`, or the SSH command in the title)
- [ ] Duplicate the last SSH command into a new pane (optional; still a new connection)
- [ ] Persist the local PTY cwd *and* the remote OSC 7 path so a restored pane starts where you launched `ssh`, with the host remembered for titles
- [ ] Treat this machine’s LAN IPs and extra hostnames as local OSC 7 hosts
- [ ] Avoid treating a remote host as local when it shares this machine’s hostname
- [ ] Resolve relative ⌘-click paths against the pane’s local cwd (not Ghostterm’s process cwd)
- [ ] SSH multiplexing (`ControlMaster`) so a new pane can attach to an existing connection without pretending the pane *is* SSH
- [ ] Open remote files (SFTP, `code --remote`, and similar) — not planned with the current local-only Finder behavior

## Test suite

Config parse/round-trip tests live in `src/config.rs` (`cargo test`). A broader suite is still to come: more unit tests around pure logic, then a few integration checks that do not need to launch the full GPUI window.

### Input

- Shift-produced characters (`:@#!` and the rest of the punctuation set) encode to the PTY
- ⌘← / ⌘→ send beginning/end of line; ⌥← / ⌥→ send word jumps
- ⌥⌫ / ⌥⌦ send word-kill; ⌘⌫ / ⌘⌦ send kill to start/end of line
- Reserved shortcuts (⌘Q / ⌘⇧T / ⌘T / ⌘W / ⌘⇧W / ⌘D / ⌘⇧D / ⌘[ / ⌘] / ⌘1–⌘9 / ⌃1–⌃9 / ⌘,) are not forwarded as terminal keys

### Links and paths

- `http://`, `https://`, and `www.` matches, including wrapped lines and trailing punctuation
- OSC 8 hyperlinks vs autodetection
- Absolute, `~/`, and `file://` paths; files resolve to the parent directory; missing paths are ignored
- `file://otherhost/...` and paths in a remote OSC 7 pane are not opened as local folders
- Hover ranges cover the full URL or path

### Sessions and window

- Top-level shell exit closes that pane, or keeps it open when `on_exit = "keep"`
- Last pane in a tab closes the tab; last tab in a session closes the session; last session closes the window without quitting the app
- Split right/down, focus next/prev, and ⌘W close the focused pane; the tree is restored from `window.toml`
- Nested `exit` (subshell) does not close the tab
- Adding and closing sessions and tabs updates the active index
- `on_exit = "keep"` restores sessions on relaunch; `on_exit = "close"` does not save them
- Session sidebar off hides the list and session shortcuts; restore keeps that one session’s tabs
- Quitting with only an unused session does not restore that tab
- ⌘N opens another window; Dock click with no windows opens one too

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
