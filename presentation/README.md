# YAS presentation

```bash
pnpm install
pnpm dev
```

The deck loads `slides/*/*.md` in path order. Each section has its own directory and each slide is one Markdown file. Optional front matter supports `layout: title` and `layout: diagram`; every diagram is a fenced Mermaid block rendered with the slide's section color.

[`SCRIPT.md`](SCRIPT.md) contains the complete slide-by-slide talk track, written for roughly 30 minutes at a conversational pace.

## Controls

- Keyboard: arrows/space, `[` and `]` for sections, `O` overview, `B` blackout, `F` fullscreen, `M` MIDI, `?` help.
- Pointer/touch: click left/right half or swipe; controls appear on pointer movement.
- Explanations: hover any title, content line, command line, or diagram for a short expansion.
- URL: every slide has a stable `/<section>/<slide>` path.

## Launchpad Mini Mk3

Click **Launchpad** or press `M`, approve Web MIDI with SysEx, and the app connects through DAW In/Out, enables DAW mode, and selects Session. The upper grid pads jump directly to slides; the bottom grid row jumps to the eight sections. Only the unlabeled 8×8 grid is used. Every edge button—including the top arrows, mode buttons, and right-side scene arrows—is untouched. Disconnecting restores Standalone mode.

The implementation follows Novation's [Launchpad Mini Mk3 Programmer's Reference Manual](https://fael-downloads-prod.focusrite.com/customer/prod/s3fs-public/downloads/Launchpad%20Mini%20-%20Programmers%20Reference%20Manual.pdf): the DAW interface, DAW/Standalone SysEx, Session layout, note/CC mapping, and palette-index LED feedback.
