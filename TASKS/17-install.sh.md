# Task 17: install.sh — One-liner installer script

## Contract

A bash script that installs octx from a GitHub release without needing cargo or rustup.

```
curl -fsSL https://octx.sh/install.sh | bash
```

## Script behavior

1. Detect OS and architecture (`uname -s` + `uname -m`)
2. Map to target triple (same mapping as `platform.rs`)
3. Construct download URL: `https://github.com/AutoMas0n/octx/releases/latest/download/octx-{target}.gz`
4. Check if already installed: if `/usr/local/bin/octx` or `~/.local/bin/octx` exists, prompt to reinstall
5. Download to temp file, verify SHA256 from the release (optional — fetch checksum file)
6. Extract gzip
7. Install to `/usr/local/bin/octx` (if writable) or `~/.local/bin/octx` (fallback)
8. Run `octx init` — adds `{data_dir}/octx/bin` to shell rc
9. Prompt: "Add a GitHub token? (Y/n)" — if yes, prompt for token, run `octx creds add github`
10. Print success message: "Done. Run `octx update` anytime to keep octx and all tools current."

## Error handling

- Exit 1 if unsupported architecture
- Exit 1 if download fails
- Check for required deps: `curl`, `gzip` (or `gunzip`), `chmod`, `mv`
- Gracefully handle existing installation (rename old binary instead of failing)

## Dependencies

- Task 14 (main.rs) — octx binary must exist to run `octx init` and `octx creds`
- But `install.sh` can be created before the binary is first released — just the script logic

## Test

- Run `shellcheck install.sh` to validate
- Manual test: `bash -n install.sh` checks syntax
- Can't unit test download + install without a real release — test the file parsing locally

## Output

- Creates: `install.sh` in the repo root
## Definition of Done
- [ ] `bash -n install.sh` reports no syntax errors
- [ ] `shellcheck install.sh` reports no warnings
- [ ] Script detects architecture and maps to target triple
- [ ] Script installs to /usr/local/bin or ~/.local/bin
- [ ] Script runs octx init after install
- [ ] Script can prompt for GitHub token via octx creds add
- [ ] Script works with curl (not wget)
- [ ] Print success message with "Run octx update to keep current"
