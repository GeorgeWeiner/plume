# plume 🪶

**A feather-light terminal IDE prototype.** Rust + [ratatui], zero heavyweight
dependencies, one binary. The look borrows from NvChad, the layout and
keybindings from VS Code, the polish ambitions from JetBrains.

> This is a working *prototype* built for speed of iteration, not a production
> editor. Everything in the keymap below actually works; the genuinely
> LSP-shaped features (semantic rename, real formatting) are honest naive
> versions or placeholders. The terminal panel, though, is a real embedded
> shell — a PTY parsed by [vt100] and rendered into the panel. Open it with
> `` Ctrl+` `` (or `Ctrl+J`); every keystroke goes to the shell, `` Ctrl+` ``
> hides it again (the shell keeps running), and the mouse wheel or
> `Shift+PageUp`/`PageDown` scrolls through its scrollback.

## Install

```sh
# installs a `plume` binary to ~/.cargo/bin (make sure it's on your PATH)
cargo install --path .
```

Then `plume` works from anywhere. Or just build and run in place:

```sh
cargo build --release
./target/release/plume
```

### Windows

`cargo install` above already puts `plume` on your PATH. For the full Windows
setup on top of that — a Start-menu and desktop shortcut plus **Open in plume**
/ **Edit with plume** in the Explorer right-click menu — run the bundled
installer from PowerShell:

```powershell
powershell -ExecutionPolicy Bypass -File windows\install.ps1
```

It installs the binary with `cargo install` (reusing the copy already on your
PATH if you have one), then adds the shortcuts and context menus. Launches
prefer Windows Terminal and fall back to the classic console, with no flashing
window. Everything is per-user, so there is no admin prompt. Skip pieces with
`-NoShortcuts` or `-NoContextMenu`, or force a rebuild with `-Reinstall`.

Undo it all with `windows\uninstall.ps1` (add `-Purge` to also delete your
config and saved sessions).

## Usage

```sh
plume                 # open the current directory as a project
plume path/to/dir     # open that directory as a project
plume a.rs b.rs       # open those files (project root = current dir)
plume notes.md        # a not-yet-existing path is created on first save
plume -r, --resume    # reopen your most recent session, from anywhere
plume -n, --new       # open fresh, without restoring a saved session
plume -h / -v         # help / version
```

Opening a folder **restores its last session** — the tabs you had open, each
file's cursor position, the sidebar state, and which tree folders were
expanded — so you pick up exactly where you left off. `--new` starts clean;
`--resume` jumps back into your most recent project regardless of where you run
it. Sessions are saved automatically on exit.

Requires a truecolor-capable terminal. Mouse is supported (click to focus,
click tabs/tree/search results, drag to select, wheel to scroll). Terminals
with the kitty keyboard protocol (kitty, foot, WezTerm, recent Konsole/Ghostty)
get the full keymap, including `Ctrl+Shift+…` combos; on legacy terminals
`Ctrl+Shift+F` may arrive as `Ctrl+F` — the command palette (`F1`) has every
command as a fallback.

## Where things are saved

Plume follows each platform's conventions (and honors `XDG_CONFIG_HOME` /
`XDG_STATE_HOME` everywhere):

| | Linux | macOS | Windows |
| --- | --- | --- | --- |
| Config (`config.toml`) | `~/.config/plume` | `~/Library/Application Support/plume` | `%APPDATA%\plume` |
| Sessions & state | `~/.local/state/plume` | `~/Library/Application Support/plume` | `%LOCALAPPDATA%\plume` |

`plume --help` prints the resolved paths on your machine.

## Keymaps

Plume ships four keymap presets — **VS Code** (default), **Visual Studio**,
**JetBrains**, and **Sublime Text**. Switch live with `Ctrl+K Ctrl+M` (or the
command palette → *Keymap: Select Preset…*); the choice is saved to your config
and restored next launch. The active keymap is shown on the right of the status
bar (`⌨ VS Code`), and the command palette lists each command's shortcut for the
current preset.

Command shortcuts vary by preset — a few examples:

| Command | VS Code | Visual Studio | JetBrains | Sublime |
| --- | --- | --- | --- | --- |
| Go to file | `Ctrl+P` | `Ctrl+,` | `Ctrl+Shift+N` | `Ctrl+P` |
| Command palette | `Ctrl+Shift+P` | `Ctrl+Shift+P` | `Ctrl+Shift+A` | `Ctrl+Shift+P` |
| Duplicate line | `Shift+Alt+↓` | `Ctrl+D` | `Ctrl+D` | `Ctrl+Shift+D` |
| Delete line | `Ctrl+Shift+K` | `Ctrl+Shift+L` | `Ctrl+Y` | `Ctrl+Shift+K` |
| Toggle comment | `Ctrl+/` | `Ctrl+K Ctrl+C` | `Ctrl+/` | `Ctrl+/` |
| Format document | `Shift+Alt+F` | `Ctrl+K Ctrl+D` | `Ctrl+Alt+L` | — |
| Rename symbol | `F2` | `Ctrl+R Ctrl+R` | `Shift+F6` | `F2` |
| Go to definition | `F12` | `F12` | `Ctrl+B` / `F12` | `F12` |

Universal keys (same in every preset): arrow/`Home`/`End`/`PgUp`/`PgDn`
movement, `Ctrl+arrows` word-jump, `Shift+…` to select, `Tab`/`Shift+Tab`
indent, `F1` command palette, `F3`/`Shift+F3` find next/prev, and `Esc` to close
a popup / find bar / panel or clear the selection. Two-key sequences (like
`Ctrl+K Ctrl+T` for the theme picker) show a pending indicator in the status bar
after the first chord.

Also universal: **`F9`** toggles the minimap, and **`F12`** jumps to the
definition of the symbol under the cursor (the JetBrains preset also maps
`Ctrl+B`). It's a naive, LSP-free jump: it looks for a definition of the symbol
(`fn foo`, `class Foo`, `let x`, `x :=`, C-style `type foo(`, …) in the current
file first, then across the project, and jumps to it — selecting the name. If
you're already sitting on the only definition, it shows the symbol's usages
instead, like JetBrains' "declaration or usages".

**In the file tree** (`Ctrl+E` to focus): `↑↓`/`jk` move, `↵` open/toggle,
`←→`/`hl` collapse/expand, `a`/`n` new file, `A` new folder, `r`/`F2` rename,
`d`/`Del` delete (confirm with `y`), `R` refresh.

The palette also exposes **Extract Variable**: select a single-line expression
and it hoists it into a `let extracted = …;` above the line.

## Minimap

A Sublime-style zoomed-out overview of the whole file runs down the right edge,
with the current viewport shown as a faint accent-tinted band. It uses
half-block glyphs for double vertical resolution and colors the overview by
syntax, so indentation and code structure are visible at a glance. Click or
drag on it to scrub through the file. Toggle it with **`F9`** (or the command
palette → *Toggle Minimap*); the choice is saved to your config (`minimap =
true`). It hides itself automatically on terminals too narrow to spare the
room, and its cost is bounded by the panel size — no slowdown on huge files.

## Indentation & bracket-pair guides

Faint dotted vertical guides run down the left edge of every indentation level,
JetBrains-style, so nested structure is readable at a glance (they even continue
through blank lines inside a block). The block that *encloses the cursor* is
emphasized: its guide is drawn in the accent color and its two brackets are
highlighted, so it's obvious which `()`, `[]` or `{}` belong together. The
bracket matcher ignores brackets inside strings and comments, picks the
innermost enclosing pair, and caps its scan.

Three modes, set with `indent_guides` in the config or cycled from the command
palette (*Indent Guides: All / Context / Off*):

- **`all`** (default) — dotted guides at every level, on every line.
- **`context`** — only the guide for the block enclosing the cursor.
- **`off`** — no guides.

Guides snap to whole character columns. `indent_guide_offset` shifts them left of
each indent stop: `0` (default) sits on the stop, `1` nudges one column into the
indentation whitespace.

## Configuration

On first run Plume writes a documented config — to `%APPDATA%\plume\config.toml`
on Windows, or `$XDG_CONFIG_HOME/plume/config.toml` (usually
`~/.config/plume/config.toml`) on Linux and macOS. It sets the default keymap
and theme, and lets you rebind any command:

```toml
# Base keymap: vscode | visual-studio | jetbrains | sublime
keymap = "jetbrains"
theme  = "Midnight Ocean"

# Rebind individual commands: action = "chord" (or "chord chord" for a sequence)
[keybindings]
duplicate_line = "ctrl+shift+d"
toggle_comment = "ctrl+shift+c"
theme_picker   = "ctrl+k ctrl+t"
```

Chords are written like `ctrl+shift+p`, `f3`, `ctrl+slash`, `alt+up`; a two-key
sequence is space-separated. Overrides sit on top of the chosen preset and
survive switching keymaps. The full action list is in the generated file's
comments. Choosing a keymap or theme from the in-app pickers rewrites just that
line, preserving your `[keybindings]`.

> Terminal note: `Ctrl+Shift+…` combos need a terminal that supports the kitty
> keyboard protocol (kitty, foot, WezTerm, Ghostty, recent Konsole). Elsewhere
> they arrive as plain `Ctrl+…`; every command is still reachable from the
> palette (`F1`).

## Themes

**57 built-in themes** — `Ctrl+K Ctrl+T` opens the picker (fuzzy-filter by name;
selection previews live, `Esc` reverts). Or set a default in the config:
`theme = "Nord"`.

Highlights include **Nord**, **Dracula**, **Gruvbox** (dark & light),
**Tokyo Night** (Night / Storm / Day), the **Catppuccin** family (Latte /
Frappé / Macchiato / Mocha), **Rosé Pine** (Main / Moon / Dawn), **Solarized**
(dark & light), **Everforest**, **Kanagawa** (Wave / Dragon / Lotus),
**Everforest**, **Ayu**, **Monokai**, **Night Owl**, **GitHub** (dark & light),
**Nightfly**, **Oxocarbon**, **Melange**, **Flexoki**, and the originals
**Midnight Ocean**, **Graphite**, and **Synthwave** — a mix of dark and light.
Every theme colors the full UI (chrome, accents, and all syntax roles), so
switching genuinely re-skins the editor.

## Architecture

```
src/
├── main.rs      terminal setup, panic-safe restore, event loop
├── cli.rs       command-line parsing (paths, --resume/--new/--help)
├── paths.rs     platform config/state directories (XDG-aware)
├── session.rs   per-project session save/restore (tabs, cursors, tree)
├── app.rs       central state: buffers, focus, overlays, commands
├── buffer.rs    text buffer: edits, undo/redo, selection, movement
├── explorer.rs  sidebar file tree (create/rename/delete/reveal)
├── keymap.rs    chords, presets (VS Code/VS/JetBrains/Sublime), parsing
├── config.rs    config file: load / persist / template
├── palette.rs   command palette / quick open / fuzzy matcher / input line
├── search.rs    project-wide grep + file listing (threaded)
├── pty.rs       embedded terminal: PTY + vt100 parser + key encoding
├── syntax.rs    hand-rolled per-line highlighter (incl. GLSL & HLSL shaders)
├── theme.rs     57 theme definitions
├── keys.rs      keyboard + mouse dispatch (keymap-driven)
└── ui.rs        all rendering
```

Design choices in the spirit of "lightweight": dependencies are kept to
ratatui plus `portable-pty` and `vt100` for the embedded terminal;
syntax highlighting is a small hand-written scanner with one line of carried
state (block comments) instead of a grammar engine; undo is delta-based (each
edit records only its changed line range) with typing coalescing; keybindings
are a data-driven table of chord-sequences resolved against a preset; the
config parser is a hand-rolled TOML subset; project search runs on worker
threads and skips binaries, `.git`, `target`, `node_modules`.

## Known prototype limits

- Rename/format/references/extract and go-to-definition are naive text
  operations, not semantic (that would need an LSP client): go-to-definition
  matches definition-shaped lines by pattern, so it can miss overloads or land
  on a same-named symbol in another scope.
- Clipboard is internal to the app (no OSC52/system clipboard).
- Files keep their original line endings (CRLF or LF) and are saved with a
  trailing newline; new files use the platform default (CRLF on Windows).
- Very large files (>1 MB) are excluded from project search.

`demo/` contains sample files in several languages to show off highlighting:
`cargo run --release -- demo`.

[ratatui]: https://ratatui.rs
[vt100]: https://crates.io/crates/vt100
