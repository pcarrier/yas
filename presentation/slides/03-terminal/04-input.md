# Browser input has more edge cases than expected

- **Keys stay keys:** controls, Alt, arrows, function keys, application-cursor mode, literal bytes.
- **IME just works:** browser composition commits the intended multi-codepoint text—not guessed keystrokes.
- **Mouse protocols work:** X10, VT200, button/any motion, SGR, pixel; down/up/move/hover/click/wheel.
- **Selection stays yours:** drag by character, word, or line even when the application owns terminal mouse mode.
- **Clipboard stays current:** live Wayland Selection when owned there; host clipboard otherwise; never a stale fallback.
- **Agents get the same controls:** send, click, mouse, resize, attach, focus, paste, pointer.
