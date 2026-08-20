# Task 16: release.yml — CI/CD pipeline

## Contract

Create a GitHub Actions workflow that:
1. Runs `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check` on push/PR
2. Runs `cargo audit` on schedule (weekly) and on push to main
3. On tag push (v*): builds release binaries for head and all arms
4. Creates a GitHub Release with all artifacts + registry-index.json

## File

`.github/workflows/release.yml`

## Jobs

### Test job (all branches)

```yaml
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: actions-rust-lang/setup-rust-toolchain@v1
    - run: cargo check
    - run: cargo test
    - run: cargo clippy -- -D warnings
    - run: cargo fmt --check
```

### Audit job (weekly + main)

```yaml
audit:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v4
    - uses: actions-rust-lang/setup-rust-toolchain@v1
    - run: cargo install cargo-audit
    - run: cargo audit
```

### Release job (tag push)

Build matrix:

| Target | Extra |
|--------|-------|
| `x86_64-unknown-linux-musl` | Default |
| `armv6-unknown-linux-gnueabihf` | For Pi Zero |
| `aarch64-unknown-linux-musl` | For Pi 3B+/4/5 |

For each target, build:
1. `octx` binary (head) — `cargo build --target ${{ matrix.target }} --release`
2. All arms: `cargo build -p octx-fmt --target ${{ matrix.target }} --release`

Then gzip each binary and attach to the release.

Also generate `registry-index.json` with the correct URLs and SHA256 hashes for all artifacts.

## Dependencies

- Task 15 (arms scaffold) — arms must exist to build

## Test

- Run by GitHub Actions on tag push. Can verify locally with `cargo build --release`
- No unit tests for a CI file

## Output

- Creates: `.github/workflows/release.yml`
- Also check `cargo install cargo-audit` works or use `cargo-audit` via `taiki-e/install-action@v2`
## Definition of Done
- [ ] Workflow parses as valid YAML
- [ ] Test job includes cargo check, test, clippy, fmt
- [ ] Audit job runs cargo audit on schedule
- [ ] Release job builds head + arms for x86_64, armv6, aarch64
- [ ] Release job generates registry-index.json with correct URLs
- [ ] CodeQL and Dependabot are configured
