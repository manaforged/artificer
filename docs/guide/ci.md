# CI

The store is plain files, so a cache step moves it between runs without a
server. `artificer export` copies the units used in the last `--days`
(default 7), newest first, under `--max-gb` (default 2). `artificer import`
adds what the runner does not have. Units carry the rustc identity, so a
different toolchain on the runner produces a cache miss. Import only
caches from trusted builds: stored artifacts are executable code.

```yaml
jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/cache@v4
        with:
          path: ~/.cargo/bin/artificer
          key: artificer-bin-${{ runner.os }}
      - run: command -v artificer || cargo install artificer-build --locked
      - run: |
          artificer install --no-modify-path
          echo "$HOME/.artificer/bin" >> "$GITHUB_PATH"
      - uses: actions/cache@v4
        with:
          path: .artificer-ci
          key: artificer-store-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}-${{ github.run_id }}
          restore-keys: |
            artificer-store-${{ runner.os }}-${{ hashFiles('**/Cargo.lock') }}-
            artificer-store-${{ runner.os }}-
      - run: |
          artificer import .artificer-ci
          cargo build --locked
          artificer export .artificer-ci --days 14 --max-gb 2
```

On the first run `.artificer-ci` does not exist, and the import is a cache
miss. `artificer install` puts the `cargo` shim in `~/.artificer/bin`, and the
`$GITHUB_PATH` line puts that directory first on PATH for later steps.
Each run saves the store under a new key. `restore-keys` restores the
newest store saved for the same `Cargo.lock`, or for any lock file when
none matches. The binary cache key does not change, so update it to
install a newer Artificer.
