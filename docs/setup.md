# Installation and setup

This guide takes you from nothing to a working, authenticated `sn`. If you just want the
short version: install with Homebrew, run `sn init`, answer the prompts, done.

- [Installation](#installation)
- [First-time setup](#first-time-setup)
- [Profiles](#profiles)
- [Non-interactive setup (CI, containers, agents)](#non-interactive-setup-ci-containers-agents)
- [API key](#api-key)
- [External bearer token](#external-bearer-token)
- [OAuth / SSO](#oauth--sso)
- [OAuth JWT bearer grant](#oauth-jwt-bearer-grant)
- [Configuration files](#configuration-files)
- [Environment variables](#environment-variables)
- [Proxy and TLS](#proxy-and-tls)

## Installation

### Homebrew (macOS / Linux)

```bash
brew install tehubersheezy/sn/sn
# or: brew tap tehubersheezy/sn && brew install sn   (upgrade later with: brew upgrade sn)
```

### Shell installer (macOS / Linux)

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/tehubersheezy/servicenow-cli/releases/latest/download/sn-installer.sh | sh
```

### Windows (MSI or PowerShell)

Download `sn-x86_64-pc-windows-msvc.msi` (64-bit Intel/AMD) or `sn-aarch64-pc-windows-msvc.msi` (ARM64 — Surface Pro X, Copilot+ PCs) from the [latest release](https://github.com/tehubersheezy/servicenow-cli/releases/latest) and double-click. For unattended/SCCM/Intune deployment use `msiexec /i sn-x86_64-pc-windows-msvc.msi /qn`. Or install via PowerShell:

```powershell
powershell -ExecutionPolicy ByPass -c "irm https://github.com/tehubersheezy/servicenow-cli/releases/latest/download/sn-installer.ps1 | iex"
```

### Pre-built binaries

Download from [Releases](https://github.com/tehubersheezy/servicenow-cli/releases): Linux (x86_64, ARM64), macOS (Intel, Apple Silicon), and Windows (x86_64, ARM64) — the latter as a portable `.zip` (no install) or `.msi` installer.

## First-time setup

`sn` supports **basic auth** (username + password) for most instances, **API keys**
(the platform's inbound REST API key, sent as the `x-sn-apikey` header), **OAuth / SSO**
for instances fronted by an external identity provider (Okta, Azure AD, ADFS), where the
password lives in the IdP and basic auth cannot work — including a secretless **JWT bearer
grant** for automation — and **external bearer tokens** that something other than `sn` issues.

For basic auth, run `sn init` and answer the prompts:

```bash
sn init
# Profile name [default]:
# Instance (e.g. 'dev380385' or 'https://acme.service-now.com'): mycompany.service-now.com
# Auth method (basic/oauth/apikey/token) [basic]:
# Username: admin
# Password: ********
# profile 'default' saved and verified (mycompany.service-now.com).
# 'default' is now the default profile.
```

`sn init` checks the credentials against the instance before saving anything, so a typo'd
password fails here rather than on your fifth command. Verify the connection any time with
`sn ping`.

## Profiles

A **profile** is a saved identity: instance + credentials + any proxy/TLS settings, under a
name. `sn init` creates one *and makes it the default* — every command uses the default
profile unless told otherwise.

To add more instances without disturbing your default, use `sn profile add`:

```bash
sn profile add prod --instance prod.service-now.com --username svc-user --auth basic   # prompts for the password
sn --profile prod table list incident --setlimit 5    # -p prod also works
sn profile use prod                  # make it the default, when you're ready
```

(Omit `--auth basic` and it prompts for the auth method too. Any field you don't pass, it
asks for — on a terminal. Off one, it fails naming the flag instead. See below.)

Profile selection is `--profile NAME` (`-p NAME`) > `default_profile` > a clear error.
There are no per-field overrides — no env var or flag substitutes a different password into
an existing profile. Change identity by rewriting the profile or selecting a different one.

## Non-interactive setup (CI, containers, agents)

`sn profile add` is built to be scripted. It never prompts when stdin isn't a terminal — it
fails naming the flag it needed — so it cannot hang a pipeline. Pipe the password in rather
than passing `--password`, which is visible in `ps` output and shell history:

```bash
sn profile add ci --instance acme.service-now.com --username svc-user --password-stdin < secret.txt
# → {"auth":"basic","default":false,"instance":"acme.service-now.com","next":"sn profile use ci",
#    "ok":true,"profile":"ci","user":"svc-user","verified":true}
```

Keys come back sorted. `user` is the identity the instance resolved the credentials to — worth
asserting on in CI, since it catches a service account being silently swapped out.

It always checks the credentials against the instance, and **a profile that fails the check is
not written at all** — no half-configured identity to trip over later. Pass `--no-verify` to
register a profile without touching the network (air-gapped provisioning, or config management
that runs before the instance is reachable).

`"next"` appears only when there's something to do about it — above, that no default profile is
selected yet, so `ci` needs `sn profile use ci` or an explicit `--profile ci`.

`add` creates; it will not silently overwrite an identity you or a teammate may be relying on:

| | |
|---|---|
| profile already exists | exit 1 — pass `--force` to overwrite |
| required flag missing, no TTY | exit 1, naming the flag |
| credentials rejected | exit 4, nothing written |
| `--non-interactive` | never prompt, even on a terminal — fail naming the flag |
| `--set-default` | also make it the default (otherwise `add` leaves it alone) |

## API key

For an instance with inbound REST API keys configured, store the key instead of a
username/password. Every request then carries it as the `x-sn-apikey` header — the
default auth parameter of the platform's API-key auth profile.

```bash
sn init --auth apikey --instance acme.service-now.com          # prompts for the key
sn profile add ci --instance acme.service-now.com --auth apikey \
  --api-key-stdin < key.txt                                    # non-interactive
```

Like every other credential, the key is verified against the instance before the
profile is saved, and it lives in `credentials.toml` (`0600`) — `sn profile show`
reports only whether one is stored, never the key itself.

**One-time admin setup** (if the instance has no key yet): **System Web Services →
API Access Policies → REST API Key → New** to generate the key, and make sure an
**inbound authentication profile** of type API Key (auth parameter `x-sn-apikey: Auth Header`)
is attached to the APIs you call via a REST API Access Policy.

## External bearer token

When something other than `sn` issues the token — an IdP minting ServiceNow-audience tokens
directly, a secrets broker, Vault, a cloud CLI — use `--auth token`. The token is sent as the
`Authorization` bearer token on every request. It comes from one of two places:

```bash
# A command sn runs (through `sh -c`, or `cmd /C` on Windows) whenever it needs a token:
sn profile add ci --instance acme.service-now.com --auth token \
  --token-command 'vault read -field=token secret/servicenow/ci'

# Or a static token, stored in credentials.toml (0600):
sn profile add ci --instance acme.service-now.com --auth token --token-stdin < token.txt
```

The command's stdout is the token: either the bare token on one line, or a JSON object with
`access_token` (also `accessToken` or `token`) and optionally an expiry — `expires_at` /
`expires_on` (Unix seconds) or `expires_in` (seconds from now). That reads the output of
`az account get-access-token -o json` and most brokers as-is. The command also sees
`SN_PROFILE` and `SN_INSTANCE` in its environment, so one script can serve several profiles.

**When the command runs again:** a token whose expiry the command stated is cached in
`credentials.toml` and reused until a minute before it expires. A token with no stated expiry
is never cached — the command runs on every `sn` invocation — because guessing a lifetime would
keep presenting a token its issuer may already have revoked. `sn profile refresh` re-runs the
command on demand; `sn profile logout` drops the cache.

The output is treated as a secret: it is never logged, never quoted in an error, and never put
on anyone's command line. The command gets no stdin (there is nobody to answer a prompt) and
must finish within `--timeout` (default 30s). If it fails, `sn` exits 1 quoting the end of its
stderr. As with every auth type, `sn profile add` verifies the token against the instance before
saving the profile.

## OAuth / SSO

Configure the profile with `sn init --auth oauth` (or `sn profile add --auth oauth`), then run the
flow with `sn profile login`:

```bash
# Authorization-code + PKCE (default): a PUBLIC client — no secret needed or prompted for.
sn init --profile sso --auth oauth --instance acme.service-now.com --client-id <id>

# Non-interactive server-to-server: client_credentials is a CONFIDENTIAL client and needs a secret
# (prompted if --client-secret is omitted; --client-secret-stdin keeps it out of `ps`).
sn profile add svc --auth oauth --instance acme.service-now.com \
  --grant client_credentials --client-id <id> --client-secret-stdin < secret.txt

sn --profile sso profile login       # run the OAuth flow, cache tokens
```

The two grants differ in whether they can be set up headlessly. `client_credentials` mints a token
without a browser, so `sn profile add` verifies it like any other credential. `authorization_code`
**requires** a browser, so there is nothing for `sn profile add` to test on a machine that has none:
it refuses rather than save an untested profile. Pass `--no-verify` to register it anyway, then have
a human run `sn profile login`.

**One-time admin setup** (if the instance has no registry entry yet): **System OAuth → Application Registry → New → "Create an OAuth API endpoint for external clients"**; set the redirect URL to `http://localhost:8400/callback` — which must match `--redirect-uri` **exactly** — and copy the client ID. For the default authorization-code flow, enable **Public Client / PKCE required** so no secret is needed; only `client_credentials` needs the generated secret.

After login, tokens refresh transparently. Manage the session with `sn profile status` (method + token expiry), `sn profile refresh`, and `sn profile logout`. The client ID and redirect URI live in `config.toml`; the secret and tokens in `credentials.toml` (both files are `0600`).

Verify any auth method at any time with `sn ping`.

## OAuth JWT bearer grant

The JWT bearer grant (RFC 7523) is the headless OAuth option with no shared secret that grants
access on its own: `sn` signs a short-lived JWT with a **private key file** and trades it at
`/oauth_token.do` for an access token. Only the key file's path is stored in `sn`'s config.

```bash
sn profile add ci --auth oauth --grant jwt_bearer --instance acme.service-now.com \
  --client-id <id> --client-secret-stdin \
  --jwt-key-file ~/.config/sn-keys/ci.pem --jwt-subject svc.integration --jwt-kid ci-key \
  < secret.txt
```

- `--jwt-key-file` — an unencrypted PEM private key: RSA (`BEGIN PRIVATE KEY` or
  `BEGIN RSA PRIVATE KEY`, at least 2048 bits) or EC P-256/P-384 (`BEGIN PRIVATE KEY`). Keep it
  `0600`. The algorithm defaults to RS256 for RSA and ES256/ES384 for EC; `--jwt-alg` picks
  RS384/RS512 instead.
- `--jwt-subject` — the `sub` claim: the user the token acts as, matched against the JWT
  endpoint's **User field** (e.g. `user_name` or `email`).
- `--jwt-kid` — the `kid` header naming the verifier map entry. Optional when the endpoint has a
  single verifier map.
- `--client-secret-stdin` — ServiceNow's JWT endpoints are confidential clients unless marked
  **Public Client**; for a public one, omit the secret.

Each assertion carries `iss` and `aud` set to the client ID, a five-minute `exp` and a fresh
`jti`, which is what the instance checks. ServiceNow issues no refresh token for this grant, so
`sn` mints a new access token from a new assertion whenever the cached one expires — no
`sn profile login` involved.

**One-time admin setup:** upload the key's X.509 certificate as a **Certificate** record
(`sys_certificate`, format PEM, type Trust Store Cert). Then **System OAuth → Application
Registry → New → "Create an OAuth JWT API endpoint for external clients"**: set the **User field**
to match your `--jwt-subject`, save, and add a **JWT Verifier Map** row pointing at the
certificate (its **Kid** is your `--jwt-kid`). The generated client ID and secret are what
`--client-id` and `--client-secret-stdin` take. To make the key and certificate:

```bash
openssl req -x509 -newkey rsa:2048 -nodes -keyout ci.pem -out ci-cert.pem -days 365 -subj "/CN=sn-ci"
```

## Configuration files

Credentials use a two-file, AWS CLI-style split:

| File | Contains | Location (Linux) |
|---|---|---|
| `config.toml` | Instance URLs, default profile, non-secret OAuth config, `token_command`, JWT key path | `~/.config/sn/` |
| `credentials.toml` | Usernames, passwords, secrets, static tokens, cached tokens | `~/.config/sn/` |
| `.sn.lock` | Empty; the advisory lock serializing config writes | `~/.config/sn/` |

macOS uses `~/Library/Application Support/sn/` and Windows `%APPDATA%\sn\`.

**Both** files are written `0600` on Unix — `config.toml` too, since it names the
instances and OAuth clients you talk to. They are created at that mode rather
than chmod'd afterwards, so a new `credentials.toml` is never briefly readable
by anyone else, and a file an older release left at `0644` is repaired on the
next write. Writes go to a temporary file in the same directory and are renamed
into place, so a crash leaves the old file rather than half of the new one.

Because that write is a read-modify-write, it is serialized by an advisory lock
on the `.sn.lock` sidecar — parallel `sn` invocations can each add a profile
without losing one another's. If another process holds the lock for more than
10 seconds, the command fails (exit 1) naming the lock file instead of hanging.

Point `sn` at a different config directory (for testing or sandboxing) with `SN_CONFIG_DIR`.

## Environment variables

| Env var | Description |
|---|---|
| `SN_CONFIG_DIR` | Override the config directory. Points **directly** at the folder holding `config.toml` and `credentials.toml` (no `sn` subdirectory appended). Cross-platform; when unset, the platform-native location is used. |
| `SN_PROXY` | HTTP/HTTPS/SOCKS5 proxy URL |
| `SN_NO_PROXY` | Comma-separated hosts to bypass the proxy |
| `SN_INSECURE=1` | Disable TLS certificate verification |
| `SN_CA_CERT` | Path to a custom CA cert for ServiceNow |
| `SN_PROXY_CA_CERT` | Path to a custom CA cert for the proxy |

```bash
SN_PROXY=http://proxy:8080 sn table list incident
SN_INSECURE=1 sn table list incident    # skip cert verification
```

There are deliberately no environment variables for credential values or profile selection — use profiles (`sn init`, `sn profile add`, `--profile`) instead. To keep a secret off the command line in a script, pipe it in with `sn profile add --password-stdin` / `--client-secret-stdin`.

## Proxy and TLS

Route through a proxy or adjust TLS per invocation:

```bash
sn --proxy http://proxy.corp:8080 table list incident   # also socks5://proxy:1080
sn --no-proxy table list incident                        # bypass a configured proxy for one call
sn --insecure table list incident                        # skip cert verification (dev/self-signed certs)
sn --ca-cert /path/to/ca.pem table list incident         # custom CA certificate
```

Any of these can live in a profile — non-secrets in `config.toml`, proxy credentials in `credentials.toml`:

```toml
# config.toml
[profiles.dev]
instance = "dev.example.com"
proxy = "http://proxy.corp:8080"
no_proxy = "localhost,127.0.0.1"
insecure = false
ca_cert = "/etc/ssl/custom-ca.pem"
proxy_ca_cert = "/etc/ssl/proxy-ca.pem"

# credentials.toml
[profiles.dev]
proxy_username = "proxy-user"
proxy_password = "proxy-pass"
```

Precedence for every proxy/TLS setting: CLI flag > env var (`SN_PROXY`, `SN_INSECURE=1`, …) > profile config.

`--insecure` is the exception: it is a logical OR across all three sources, not a chain. TLS verification is disabled if **any** of the flag, `SN_INSECURE`, or the profile's `insecure = true` says so — there is no way to turn it back *on* for one invocation of a profile that has it set. That's deliberate (a footgun should not be quietly re-armed by a stale config), but it means the only way to undo `insecure = true` is to edit the profile.
