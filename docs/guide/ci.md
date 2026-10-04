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

## Share units between machines

A first build on a new machine or a fresh checkout can reuse units that
another machine already compiled. Units only match on the same platform and
toolchain, so pick one builder machine per platform. On every other machine
of that platform, point Artificer at the builder's store:

```sh
artificer install --remote builder:/path/to/its/store
```

The location is `HOST:/ABSOLUTE/PATH`, reached over SSH, or an absolute
directory such as a mounted share. The path is the builder's Artificer
store, the directory `artificer stat` prints. Install records the remote and
starts the first pull in the background. `artificer remote set LOCATION`
changes it later, `artificer remote off` clears it, and `ARTIFICER_REMOTE`
overrides it for one shell.

`artificer pull` copies the complete units this store does not have. Over
SSH it runs one `rsync`, so `rsync` and `ssh` must be installed and the
host must accept key login without a prompt. On Windows, use a directory remote. After that, handled builds
start a background pull when the last one is more than 15 minutes old and
write its output to `pull.log` in the store. Builds never wait on the
network. Pull only from machines you trust: stored artifacts are executable
code.
