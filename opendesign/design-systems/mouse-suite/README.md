# Mouse Suite design system

Desktop-adapted from `design-system/mouse-suite/MASTER.md`.

| Source | Notes |
|--------|--------|
| MASTER.md | Teal `#0D9488` + CTA orange `#EA580C` |
| `src/theme.rs` | Existing egui structure (light/dark, chrome, panels) |

## Contents

- `tokens/colors_and_type.css` — light + dark CSS variables
- `SKILL.md` — agent usage rules

## Mapping to egui `Palette`

| CSS | Palette field |
|-----|---------------|
| `--accent-1` | `ACCENT` |
| `--cta` | `ACCENT_HOT` (primary CTA fill when needed) |
| `--bg-1` | `BG` |
| `--bg-panel` | `PANEL` |
| `--fg-1` | `TEXT` |
