# I wanted the window, not somebody else's desktop

- **One resource per window:** title, `app_id`, origin, parent, size, scale, lifecycle, cursor, text input, activation.
- **Know what launched it:** sandbox engine + stable app ID + instance ID; Terminal/Process launch into app endpoints.
- **Show real chrome:** live titles and lazy `.desktop` icons from `@session`; server-stamped identity beats self-report.
- **Keep the tree together:** popups and subsurfaces composite into their top-level stream; top levels get panes.
- **Ask for attention—never steal it:** activation marks the window instead of hijacking focus.
- **Control the app:** list, switch, search by title/app; focus, resize, close, capture, record.
