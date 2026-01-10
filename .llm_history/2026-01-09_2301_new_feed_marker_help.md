Changes on 2026-01-09

- Added transient new feed marker rendering: a green '*' prefix appears for newly fetched feeds for 1 minute, starting only after the app has been running for 60 seconds.
- Updated help modal to document the indicator:
  - Added "Indicators:" section with explanation of the '*' new feed marker behavior.
- Implementation details:
  - Tracked `start_time: Instant` and `new_feed_marks: HashMap<u64, Instant>` in `AppState`.
  - On each data update, if uptime >= 60s, mark unseen IDs with expiry of now + 60s.
  - Prune expired marks during draw; render '*' when mark is active.
