---
paths:
  - "src/cli/init.rs"
  - "src/cli/profile.rs"
  - "src/config.rs"
  - "tests/init.rs"
  - "tests/profile_*.rs"
---

# Profiles: resolution, `sn init` vs `sn profile add`

## Modules

- `src/cli/init.rs` → sn init (onboarding wizard: profile setup + verification, and ALWAYS claims default_profile); a thin policy layer over profile.rs's core
- `src/cli/profile.rs` → sn profile add/list/show/remove/use plus login/logout/status/refresh (dispatched to auth.rs) + the shared profile-writing core (ProfileInput, resolve_name/resolve_input, save_and_verify) used by both `add` and `init`

## Profile resolution precedence

A profile is the single unit of identity: commands either manage profiles (`sn init`, `sn profile *`) or use exactly one. Selection is `--profile` flag > `default_profile` in config.toml; if neither resolves, `resolve_profile_name` (in `config.rs`) returns `Error::Config("no profile selected; pass --profile <name> or run \`sn init\`")` (exit 1). There are **no** per-field argv overrides — change identity by rewriting the profile (`sn init`, or `sn profile add --force`) or selecting a different one via `--profile`. A profile with an empty/whitespace `instance` is rejected. There are deliberately no env vars for credential values or profile selection; proxy/TLS env vars (`SN_PROXY` etc.) and the `SN_CONFIG_DIR` config-directory override (see below) still exist.

## `sn init` vs `sn profile add`

Both write a profile through **one shared core** in `cli/profile.rs` (`resolve_name` → `resolve_input` → `save_and_verify`); they differ only in the `SavePolicy` fields layered on top, so the two can't drift:

| | `sn init` | `sn profile add` |
|---|---|---|
| role | onboarding wizard | scriptable creator |
| `default_profile` | **always** claims it | **never** touches it (unless `--set-default`) |
| existing profile | upserts | exit 1 unless `--force` |
| stored proxy/TLS a flag omits (`replace_connection`) | kept (merge; `--no-proxy` clears `proxy`) | cleared — `--force` rebuilds `proxy`/`insecure`/`ca_cert`/`proxy_ca_cert` from argv, so re-adding without `--insecure` is how it's turned off. `no_proxy` has no flag and survives both |
| result reported as | human text on stderr | JSON on stdout; verification can add stderr diagnostics |
| secret input | prompt or explicit secret flags | `--password-stdin` / `--client-secret-stdin` / `--api-key-stdin` / `--token-stdin`, prompt, or explicit flags |
| skip verification | can't | `--no-verify` |

(Prompts themselves go to **stdout** — `print!` in `profile::ask` — for both. They only ever fire on a TTY, so scripted stdout stays clean JSON.)

`sn profile show`, `sn profile list` and `add`'s result all report the stored proxy/TLS fields through `profile::connection_fields`, under their `config.toml` names: `insecure` **always** (its absence is what made `--insecure` look like it never stuck — issue #97), the rest only when set. The proxy URL goes through `redact_url_password`, which masks an embedded password *in place* rather than re-serializing (`Url` would append a `/` the file doesn't hold); proxy credentials in `credentials.toml` are never read.

The shared core speaks `profile add`'s vocabulary, so a `Caller` (`Init` / `ProfileAdd`) is threaded through `resolve_name`/`resolve_input`: every missing-field message must name a flag the *invoked* command actually has. Without it `sn init` on a non-TTY pointed at `--password-stdin`, which `init` does not accept.

Verification is on by default and `save_and_verify` **rolls the config files back** when it fails — a profile that can't authenticate must never survive on disk, or it fails later somewhere confusing. `sn profile add --no-verify` is the one deliberate escape hatch (air-gapped provisioning; registering an `authorization_code` profile a human will later `sn profile login` to); `sn init` has no such flag and always verifies. Prefer the `-stdin` secret flags over argv, which `ps` and shell history can see.
