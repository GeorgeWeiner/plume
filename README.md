# plume 🪶

**A feather-light terminal IDE prototype.** Rust + [ratatui], zero heavyweight
dependencies, one binary. The look borrows from NvChad, the layout and
keybindings from VS Code, the polish ambitions from JetBrains.

> This is a working *prototype* built for speed of iteration, not a production
> editor. Everything in the keymap below actually works; the genuinely
> LSP-shaped features (semantic rename, real formatting, the terminal panel)
> are honest naive versions or placeholders.

## Build & run

```sh
cargo build --release

# open a project directory
./target/release/plume demo

# or open whatever directory you're in
cargo run --release

# or open a single file (its parent dir becomes the project)
cargo run --release -- demo/main.rs
```

Requires a truecolor-capable terminal. Mouse is supported (click to focus,
click tabs/tree/search results, drag to select, wheel to scroll). Terminals
with the kitty keyboard protocol (kitty, foot, WezTerm, recent Konsole/Ghostty)
get the full keymap, including `Ctrl+Shift+…` combos; on legacy terminals
`Ctrl+Shift+F` may arrive as `Ctrl+F` — the command palette (`F1`) has every
command as a fallback.

## Keymap

| Key | Action |
| --- | --- |
| `Ctrl+P` | Quick open (fuzzy file finder) |
| `Ctrl+Shift+P` / `F1` | Command palette |
| `Ctrl+Shift+F` | Search in project (results panel, `↵` jumps) |
| `Ctrl+F` | Find in file (live highlight, `↵`/`F3` next, `Shift+F3` prev) |
| `Ctrl+G` | Go to line |
| `Ctrl+S` | Save (`Save As` prompt for untitled buffers) |
| `Ctrl+N` | New untitled file |
| `Ctrl+W` | Close tab (warns once on unsaved changes) |
| `Ctrl+PgUp` / `Ctrl+PgDn` | Previous / next tab |
| `Ctrl+B` | Toggle sidebar |
| `Ctrl+E` | Focus file explorer (again to return) |
| ``Ctrl+` `` / `Ctrl+J` | Toggle terminal panel (placeholder) |
| `Ctrl+K Ctrl+T` | Theme picker (live preview) |
| `F2` | Rename symbol — naive whole-word rename, current file |
| `Shift+Alt+F` | Format document — trims trailing whitespace |
| `Shift+F12` | Find references — project-wide word search |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| `Ctrl+C` / `Ctrl+X` / `Ctrl+V` | Copy / cut / paste (internal clipboard) |
| `Ctrl+A` | Select all |
| `Ctrl+D` | Duplicate line |
| `Alt+↑` / `Alt+↓` | Move line up / down |
| `Ctrl+/` | Toggle line comment |
| `Shift+arrows`, `Ctrl+arrows` | Select, move by word |
| `Tab` / `Shift+Tab` | Indent / dedent selection |
| `Esc` | Close popup / find bar / panel, clear selection |

**In the file tree** (`Ctrl+E` to focus): `↑↓`/`jk` move, `↵` open/toggle,
`←→`/`hl` collapse/expand, `a`/`n` new file, `A` new folder, `r`/`F2` rename,
`d`/`Del` delete (confirm with `y`), `R` refresh.

The palette also exposes **Extract Variable**: select a single-line expression
and it hoists it into a `let extracted = …;` above the line.

## Themes

`Ctrl+K Ctrl+T` — selection previews live, `Esc` reverts:

- **Midnight Ocean** — deep blue (default)
- **Graphite** — neutral OneDark gray
- **Solar Dawn** — clean light theme
- **Synthwave** — neon retrowave

## Architecture

```
src/
├── main.rs      terminal setup, panic-safe restore, event loop
├── app.rs       central state: buffers, focus, overlays, commands
├── buffer.rs    text buffer: edits, undo/redo, selection, movement
├── explorer.rs  sidebar file tree (create/rename/delete/reveal)
├── palette.rs   command palette / quick open / fuzzy matcher / input line
├── search.rs    project-wide grep + file listing
├── syntax.rs    hand-rolled per-line highlighter (12 languages)
├── theme.rs     theme definitions
├── keys.rs      keyboard + mouse dispatch
└── ui.rs        all rendering
```

Design choices in the spirit of "lightweight": the only dependency is ratatui;
syntax highlighting is a small hand-written scanner with one line of carried
state (block comments) instead of a grammar engine; undo is snapshot-based
with typing coalescing; project search walks the tree directly and skips
binaries, `.git`, `target`, `node_modules`.

## Known prototype limits

- Rename/format/references/extract are naive text operations, not semantic
  (that would need an LSP client).
- The terminal panel is a visual placeholder — no PTY.
- Clipboard is internal to the app (no OSC52/system clipboard).
- Files are saved with `\n` line endings and a trailing newline.
- Very large files (>1 MB) are excluded from project search.

`demo/` contains sample files in several languages to show off highlighting:
`cargo run --release -- demo`.

[ratatui]: https://ratatui.rs
