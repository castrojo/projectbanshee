# 06: Ctrl+Left/Right skip tracks instead of moving by word in text fields

**What to build:** `win.next` = `<Control>Right` and `win.previous` = `<Control>Left` (`src/main.rs:213-214`). GTK 4.20 runs application accels from a capture-phase, global-scope shortcut controller (`gtk/gtkwindow.c:2985-2989`), so they take precedence over GtkText's Ctrl+Left/Right word-move bindings (`gtk/gtktext.c:1428,1431`). The Search entry is focused at launch and after every Enter, so word navigation there skips tracks instead. The Discord Application ID row is affected the same way.

This is a keybinding change that users will notice. Pick replacement accels (GNOME Music uses Ctrl+N / Ctrl+B), or keep the current ones and drop them only while a text widget has focus.

**Blocked by:** None

**Status:** ready-for-human

**GitHub:** https://github.com/castrojo/projectbanshee/issues/6

- [ ] Decide the replacement accels
- [ ] Update the accels and the shortcuts dialog (`src/main.rs:173-174`) and README

## Comments

