# Install

## Install

You need Rust 1.98 or later, installed with rustup. When Artificer hands a
command to real Cargo, rustup still picks the toolchain from each
repository's `rust-toolchain` file. Install cargo-nextest only if you use it.

```sh
cargo install artificer-build --locked
artificer install
```

To install the latest commit instead, use
`cargo install --git https://github.com/manaforged/artificer --locked`.
From a clone, `./scripts/install.sh` (macOS, Linux) or
`.\scripts\install.ps1` (Windows) builds the locked graph and runs
`artificer install` for you.

`artificer install` does four things:

1. Copies `artificer` to `CARGO_HOME/bin` if it is not already there.
2. Puts the `cargo` shim in `~/.artificer/bin`.
3. Records the path of the real Cargo in `~/.artificer/real-cargo`.
4. Adds this line to the end of `~/.profile`, of `~/.zshenv` when your
   `SHELL` is zsh or that file exists, and of `~/.bashrc` and
   `~/.bash_profile` if they exist:

   ```sh
   [ ! -f "$HOME/.artificer/env" ] || . "$HOME/.artificer/env"
   ```

`~/.zshenv` is the one zsh file that every zsh reads, including
`zsh -c` from an editor or a script. `~/.profile` covers
login shells such as `bash -lc`. If you set `ZDOTDIR`, the line goes to
`$ZDOTDIR/.zshenv`. Running `artificer install` again does not add a second
copy.

To manage your profiles yourself, run `artificer install --no-modify-path`
and add the printed line where you want it.

On Windows, `artificer install` puts `%USERPROFILE%\.artificer\bin` first on
your user PATH (`HKCU\Environment\Path`), the way rustup adds its own
directory. New terminals pick it up. Windows puts the system PATH before the
user PATH. If Rust is installed for all users, so that Cargo is on the
system PATH, add the shim directory to the front of the system PATH too.

## Check the installation

Open a new shell, then run:

```sh
artificer doctor
```

No row may read `BAD`. A `note` row is information:
`serve` is down, `fallbacks` counts fallback commands, and `analyzer`
reports the rust-analyzer CLI config.

The shim must come before any other Cargo on PATH. If `doctor` reports
another Cargo first, a later line in your shell startup files puts a Cargo
directory in front of `~/.artificer/bin`. Move the Artificer line after it.

When `rustc` is not on PATH, the shim uses the `rustc` next to the real
Cargo. A minimal PATH such as `/usr/bin:/bin` plus the shim still builds.

## Editor

Set `CARGO` for rust-analyzer to the shim path. Until your editor runs the
shim, editor checks build into a separate cache.

For a user-level setting, add this to `rust-analyzer.toml` in your user
configuration directory, such as `~/.config/rust-analyzer/`. Replace
`<HOME>` with your home directory; `doctor` checks
`~/.config/rust-analyzer/rust-analyzer.toml`:

```toml
[cargo]
extraEnv = { CARGO = "<HOME>/.artificer/bin/cargo" }
```

In VS Code, put the same value in `settings.json` as
`"rust-analyzer.cargo.extraEnv": { "CARGO": "<HOME>/.artificer/bin/cargo" }`.

## Scripts and CI

A process started from a shell that reads your profile finds the shim with
no extra setup. A process started with an explicit environment needs
`~/.artificer/bin` first on its PATH, or `CARGO` set to
`~/.artificer/bin/cargo`. To carry the cache between CI runs, see
[CI](ci.md).

## Uninstall

```sh
artificer uninstall
```

This stops the daemon, removes the shim, and removes the PATH line from
every profile it was added to, or the user PATH entry on Windows. The cache stays. Add `--purge` to delete the
cache too. If Cargo installed Artificer, uninstall then runs
`cargo uninstall artificer-build`. On Windows it prints that command, or you
can run `.\scripts\install.ps1 -Uninstall` from a clone, which also removes
the running binary after it exits.

Remove the `CARGO` setting from your editor if you added one. The
[reference](reference.md#uninstall) has the details.
