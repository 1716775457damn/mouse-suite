---
name: mouse-suite-design-system
description: Desktop product design system for Mouse Suite — teal primary, orange CTA, neutral tool chrome, light/dark.
---

# Mouse Suite Design System

Use for in-app UI mockups and egui theme porting. Not for marketing landing pages.

## Tokens

Load `tokens/colors_and_type.css`. Toggle dark with `data-theme="dark"` on `<html>`.

## Rules

- Density: medium-high tool UI (sidebars ~200–220px, chrome ~82px).
- Primary actions: teal (`--accent-1`). Destructive / capture / run CTAs may use `--cta` (orange) sparingly.
- Panels: neutral gray chrome; mint (`--bg-tint`) only as a light wash, never full-page mint.
- Radius 6–10px; shadows soft; transitions 150–200ms; no layout-shifting hovers.
- Fonts in mockups: Plus Jakarta Sans. Runtime egui: YaHei + Segoe.
