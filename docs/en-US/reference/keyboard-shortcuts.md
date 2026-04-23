---
title: Keyboard Shortcuts
---

# Keyboard Shortcuts

Most editor shortcuts are customizable from **Settings > Keybinds**. The defaults below match a fresh install.

## Canvas controls

These are gesture-based and not user-rebindable.

| Shortcut | Action |
| --- | --- |
| `Ctrl` + mouse wheel | Zoom in or out |
| `Ctrl` + drag | Pan the canvas |
| Pinch on trackpad | Pinch-zoom |

## Tool switching

| Default | Action |
| --- | --- |
| `V` | Switch to the Select tool |
| `M` | Switch to the Block tool (text-block creation) |
| `B` | Switch to the Brush tool |
| `E` | Switch to the Eraser tool |
| `R` | Switch to the Repair Brush tool |

## Brush size

| Default | Action |
| --- | --- |
| `]` | Increase brush size (clamped at 128) |
| `[` | Decrease brush size (clamped at 8) |

## History and selection

| Default | Action |
| --- | --- |
| `Ctrl` + `Z` (`Cmd` + `Z` on macOS) | Undo |
| `Ctrl` + `Shift` + `Z` (`Cmd` + `Shift` + `Z` on macOS) | Redo |
| `Ctrl` + `Y` | Redo (legacy fallback, not rebindable) |
| `Ctrl` + `A` (`Cmd` + `A` on macOS) | Select all text blocks on the current page |

Undo and redo intentionally fire even while typing in a text field — the scene history takes precedence over the browser's native text-undo. `Ctrl + A` only fires outside text fields, so the native "select all text" behaviour still works inside textareas and inputs.

## Customizing shortcuts

Open **Settings > Keybinds** to rebind any of the rebindable shortcuts above. Conflicts are highlighted, and you can reset back to defaults from the same screen.

Tool-switch and brush-size shortcuts only fire when the keyboard focus is outside an editable text field, so they will not interrupt typing in the OCR or translation panels.
