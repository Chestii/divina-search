# divina-search

A tiny always-on-top launcher. Hit Super+S from anywhere, type part of a
filename, hit enter. Falls back to a web search if nothing matches.

Built with [iced](https://iced.rs) — wanted a native black search bar
without dragging in a browser runtime for something this small.

## Running

```
cargo run --release
```

Registers a global hotkey on startup (Super+S, falling back to Ctrl+Alt+S
or Ctrl+Shift+S if that combo's already taken by something else). Sits
hidden until you press it.

## What it searches

Desktop, Documents, Downloads, Pictures, Videos — walked a few levels deep
on every keystroke, skipping dotfiles and the obvious junk folders
(node_modules, target, and so on). It's not an index, just a live walk
capped at a few thousand entries so it doesn't hang on huge folders.

- Enter opens the highlighted result, or runs a web search if the web
  entry is selected
- Up / Down moves the selection
- Escape hides the window
