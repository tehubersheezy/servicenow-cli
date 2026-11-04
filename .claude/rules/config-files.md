---
paths:
  - "src/config.rs"
  - "tests/config_atomic.rs"
  - "tests/env_var_isolation.rs"
---

# Config file locations and persistence

## Modules

- `src/config.rs` → Config/Credentials TOML types, load/save, resolve_profile(); OAuth types (AuthMethod, OAuthConfig, OAuthGrant, TokenSet) + token persistence

Resolved by `config::config_dir()`. A non-empty `SN_CONFIG_DIR` is used as-is — the documented cross-platform override pointing **directly** at the directory holding `config.toml`/`credentials.toml` (no `sn` subdir appended). Otherwise `directories::ProjectDirs::from("", "", "sn")` gives the platform-native path: Linux `~/.config/sn/`, macOS `~/Library/Application Support/sn/`, Windows `%APPDATA%\sn\`.

`SN_CONFIG_DIR` is the cross-platform config-isolation mechanism for tests, so config-dependent integration tests are not `#[cfg(target_os = "linux")]`-gated. Point it at a temp dir containing the seeded config files (never a `/sn` subdir).

**Both files are written atomically at 0600.** `write_atomic` puts the contents in a `tempfile` created **0600 by `open(2)` itself** — never `chmod`'d afterwards, because create-then-chmod is exactly the window this path exists to close: `save_credentials_to` used to write and then chmod, so a brand-new `credentials.toml` sat at the umask default with a password already inside. The temp file is in the **same directory** (`rename(2)` is only atomic within one filesystem, so `/tmp` would be a copy), `sync_all`'d before the rename so the name cannot publish blocks that were never written, and the containing directory is `fsync`'d after it (best-effort, Unix-only) so the rename itself is durable. `config.toml` gets the same treatment and the same **0600** — it was 0644, and it names the instances and OAuth clients a user talks to, and nothing else needs to read it. The rename carries the temp file's mode across, so this also repairs files an older release left at 0644.

**The advisory lock lives on a `.sn.lock` sidecar, never on the config files themselves.** They are replaced by rename, so a lock taken on one of them is a lock on an inode the next writer will never open — every writer would hold "the lock" on a different inode and none would block. Only a file created once and never renamed over has a stable identity to lock.

- **One lock covers the whole directory**, not one per file. The two files are written as a pair, and a single lock lets a writer hold that pair across the whole read → modify → write. It also means there is no lock ordering, and so no deadlock.
- **Acquisition is re-entrant per thread.** `flock(2)` locks an open file *description*, so a second `open` + lock from the same thread contends with the first and would deadlock the process against itself (`update_credentials_at` → `save_credentials_to` is exactly that shape). `HELD_LOCKS` records held directories per thread, keyed on the **canonicalized** path so `cfg` and `./cfg` cannot look like two locks. Contention between threads and processes is untouched — each opens its own description.
- **Contention is bounded by a 10s `LOCK_TIMEOUT`, not an unbounded block**, so an agent gets a diagnosable exit 1 (`timed out … another sn process is holding it`) instead of an invocation that never returns. `with_config_lock_for` takes a caller-chosen budget for the one critical section that is not "parse, mutate, write": `oauth::ensure_access_token` holds the lock across the `/oauth_token.do` round trip, so it passes a budget derived from the HTTP timeout — with the default 10s every parallel invocation would fail with a spurious lock timeout the moment the IdP got slow.
- **Any load → modify → save must go through `config::with_config_lock`.** The `save_*` functions lock only their own write, which does nothing about two processes that each read the old file first: twelve parallel `sn profile add`s left **one** profile on disk while eleven exited 0 reporting success for a profile that no longer existed. `save_profile`/`restore`/`remove`/`set_default` run their whole sequence inside it; the nested `save_*` acquisition is the re-entrant no-op above. Emission stays *outside* the lock so a slow pipe cannot park other writers, and verification stays outside it because it is a network round trip that can involve a browser and a human.
- **Readers take no lock** — every invocation reads the config, and contending would trade a rare lost update for a common stall. Write *ordering* is what keeps them honest instead: adding writes `credentials.toml` then `config.toml`, so a reader that sees the new config entry is guaranteed to see the credentials too (the interleaving that yields `Auth::Basic("", "")` and a baffling 401 is arithmetically impossible, not merely unlikely); removal goes the other way. Removal's residue is one two-`read(2)` window that only a reader-side shared lock would close.

The same directory also holds `number_prefixes.toml`, `sn get`'s record-number prefix cache (see "Record references" under Conventions). It shares the lock and the atomic 0600 write, but is read leniently — a missing or corrupt cache is an empty one — because losing it costs one request.

`fs4` provides the lock (maintained fork of the dead fs2; wraps `flock(2)`/`LockFileEx` behind one safe API; std's `File::lock` would have cost MSRV 1.89 — note `lock_dir` spells it `fs4::FileExt::try_lock(&file)` as a trait call so 1.89's inherent method cannot silently shadow it). `tempfile` moved from dev-dependencies to dependencies because the write path now uses it at runtime.
