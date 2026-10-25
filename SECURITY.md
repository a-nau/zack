# Security

Zack is a local desktop tool. Treat its configuration as executable input:
configured actions can start programs, open URLs and paths, and inject keys
into the focused application. Load settings only from sources you trust.

Keep private hosts, paths, workspace identifiers and personal shortcuts in
`mappings.local.json`. The overlay is ignored by Git but is still an ordinary
local file; excluding it from Git does not encrypt it or prevent backups.
Zack's logs can contain URLs, command arguments and foreground application
details. Inspect and redact logs and screenshots before sharing them.

Accessibility access on macOS and keyboard/input access on Linux are powerful
permissions. The macOS installer creates a local signing identity to preserve
Accessibility grants across local rebuilds. This is not a publisher signature
or notarization. Review install scripts before running them.

The Windows updater trusts the configured Git upstream and builds its code
locally. Keep that checkout and its scripts writable only by trusted users.

This is an experimental project with no guaranteed support period or response
time. Do not post credentials, private configuration, or undisclosed
vulnerability details in public issues.
