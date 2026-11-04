# Changelog

## 1.0.0

First release.

- Clipboard history for text and pictures, newest first, with the same copy moved to the top instead of saved twice.
- Full-text search (SQLite FTS5) with highlighted matches, and time filters: today, 3 days, this week, 30 days.
- Every clip is sorted into a kind: link, email, colour, number, file path, code, text or picture, each with its own preview.
- Pins and folders. Pinned clips and clips in folders are never cleaned up.
- Snippets with keywords and placeholders: `{date}`, `{time}`, `{datetime}`, `{weekday}`, `{year}` and `{clipboard}`.
- Quick paste window behind a global shortcut (Ctrl+Alt+V, or Cmd+Shift+V on macOS) that pastes straight into the app you were in.
- Passwords, API keys, tokens and card numbers are spotted and skipped (or kept masked for 15 minutes).
- Ignores copies from password managers and from apps you list. On Windows it also respects the "don't record this copy" flag that password managers set.
- Optional encryption: Argon2id with XChaCha20-Poly1305, and search still works while unlocked.
- Pause recording, tray menu, start with your computer, start hidden, light and dark themes, export to JSON.
