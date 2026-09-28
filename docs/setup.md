# Installation and setup

This guide takes you from nothing to a working, authenticated `sn`. If you just want the
short version: install with Homebrew, run `sn init`, answer the prompts, done.

- [Installation](#installation)
- [First-time setup](#first-time-setup)
- [Profiles](#profiles)
- [Non-interactive setup (CI, containers, agents)](#non-interactive-setup-ci-containers-agents)
- [API key](#api-key)
- [OAuth / SSO](#oauth--sso)
  - [Create an OAuth Application Registry entry](#create-an-oauth-application-registry-entry)
  - [Connect sn with your client ID](#connect-sn-with-your-client-id)
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
(the platform's inbound REST API key, sent as the `x-sn-apikey` header), and **OAuth / SSO**
for instances fronted by an external identity provider (Okta, Azure AD, ADFS), where the
password lives in the IdP and basic auth cannot work.

For basic auth, run `sn init` and answer the prompts:

```bash
sn init
# Profile name [default]:
# Instance (e.g. 'dev380385' or 'https://acme.service-now.com'): mycompany.service-now.com
# Auth method (basic/oauth/apikey) [basic]:
# Username: admin
# Password: ********
# profile 'default' saved and verified (mycompany.service-now.com).
# 'default' is now the default profile.
```

`sn init` checks the credentials against the instance, so a typo'd password fails during
setup. If verification fails, it restores the previous profile
configuration. Verify the connection any time with `sn ping`.

## Profiles

A **profile** is a saved identity: instance + credentials + any proxy/TLS settings, under a
name. `sn init` creates one *and makes it the default*. Commands that connect to an
instance use that profile unless you select another with `--profile`.

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

By default it checks the credentials against the instance and **rolls back the profile
changes if verification fails**. Pass `--no-verify` to
register a profile without touching the network (air-gapped provisioning, or config management
that runs before the instance is reachable).

`"next"` appears only when there's something to do about it — above, that no default profile is
selected yet, so `ci` needs `sn profile use ci` or an explicit `--profile ci`.

`add` creates; it will not silently overwrite an identity you or a teammate may be relying on:

| | |
|---|---|
| profile already exists | exit 1 — pass `--force` to overwrite (proxy/TLS settings are rebuilt from that run's flags) |
| required flag missing, no TTY | exit 1, naming the flag |
| credentials rejected | exit 4, profile changes rolled back |
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

The key is verified against the instance, and profile changes are rolled back if
verification fails. It lives in `credentials.toml` (`0600`); `sn profile show` reports
only whether one is stored, never the key itself.

**One-time admin setup** (if the instance has no key yet): **System Web Services →
API Access Policies → REST API Key → New** to generate the key, and make sure an
**inbound authentication profile** of type API Key (auth parameter `x-sn-apikey: Auth Header`)
is attached to the APIs you call via a REST API Access Policy.

## OAuth / SSO

For browser login, `sn` uses the authorization-code flow with PKCE. You sign in through
ServiceNow in your browser, including your usual SSO provider if the instance uses one.
We recommend a dedicated OAuth **client ID** on the instance you want to connect to.
The following steps create one and configure `sn` to use it.

### Create an OAuth Application Registry entry

This is a one-time setup on the ServiceNow instance. Ask your instance administrator to
do it if you don't have permission to manage OAuth applications. The ServiceNowDocs
Brazil documentation lists `oauth_admin` for Application Registry; the required role
and form layout can differ on older releases.

1. Sign in to the ServiceNow instance in your browser.
2. Open **All**, search for **Application Registry**, and select
   **System OAuth → Application Registry**.
3. Click **New**, then **Create an OAuth API endpoint for external clients**.
4. Fill in the form using the settings below.

| Field | Value for `sn` browser login |
|---|---|
| **Name** | A recognizable name, such as `sn CLI` |
| **Client ID** | Keep the value ServiceNow generates |
| **Redirect URL** | `http://localhost:8400/callback` |
| **Active** | Checked |
| **Public Client** | Checked, so the client can authenticate with PKCE without a client secret |
| **Client Secret** | Leave blank; `sn` doesn't need a secret for a public client, even if the form generates one |
| **Client Type**, if shown | **Integration as a User** |

5. Click **Submit** to save the entry.
6. Reopen it from the Application Registry list and copy the **Client ID** field.
   Use this value for `--client-id` in the next section. The record's `sys_id` and
   the Client Secret are different values.

The redirect URL is where the browser returns control to `sn` on your computer after
login. Use the exact URL above, including `http`, the port, and `/callback`. If you
choose another port, save that URL in the registry and pass the same URL to
`sn init --redirect-uri`. Run the CLI and browser on the same computer for this setup.

**If your instance uses Machine Identity Console:** the newer setup path is
**Machine Identity Console → Inbound integrations → New integration → OAuth - Authorization
code grant**. Set **Name of OAuth entity** and the required **Provider name** to a
recognizable value such as `sn CLI`, enter the same redirect URL, select **Active** and
**This is a public client**, then **Save** and copy the **Client ID**. Have your
administrator configure auth scopes and API access policies for the APIs you intend
to call. The account you sign in with still needs permission to read or change those
records.

These steps use the ServiceNowDocs repository's Brazil documentation for
[Application Registry](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/platform-security/authentication/t_CreateEndpointforExternalClients.md)
and the newer [authorization-code setup form](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/platform-security/authentication/configure-an-oauth-authorization-code-grant.md).
ServiceNow describes public clients and PKCE in its
[authorization-code grant overview](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/platform-security/authentication/authorization-code-grant.md).

### Connect sn with your client ID

Replace `acme.service-now.com` with your instance and `YOUR_CLIENT_ID` with the value
you copied:

```bash
sn init --profile sso --auth oauth \
  --instance acme.service-now.com \
  --client-id YOUR_CLIENT_ID \
  --redirect-uri http://localhost:8400/callback
```

`sn init` opens the browser for login. Sign in as the user you want the CLI to act as,
then approve the request. The browser returns to the local callback, and `sn` caches
the tokens, verifies the connection, and makes `sso` your default profile. No client
secret is needed, and PKCE is enabled automatically.

Check the connection:

```bash
sn --profile sso ping
```

Use `sn --profile sso profile login` when you need to sign in again. To add a profile
without changing your default, use `sn profile add sso --auth oauth` with the same
instance, client ID, and redirect URI flags.

If login fails, check that the registry entry is active, the client ID belongs to the
same instance, the client is public, and the redirect URLs match exactly. If `sn`
can't bind port 8400, close the other login attempt or configure another port in both
places.

### OAuth without a browser

For server-to-server use, create a separate confidential client for the
`client_credentials` grant. This needs both a client ID and a client secret. In the
Application Registry workflow, the administrator must enable the client-credentials
system property and associate an **OAuth Application User** with the client. See
[ServiceNow's client-credentials setup](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/platform-security/authentication/client-credentials.md)
and [application-user instructions](https://github.com/ServiceNow/ServiceNowDocs/blob/brazil/markdown/platform-security/authentication/add-oauth-application-user.md),
including the REST API Auth Scope requirement.

```bash
sn profile add svc --auth oauth --instance acme.service-now.com \
  --grant client_credentials --client-id YOUR_CLIENT_ID --client-secret-stdin < secret.txt
```

This verifies the profile without a browser. `--client-secret-stdin` reads the secret
from the file instead of putting it in the command line.

To save an authorization-code profile on a machine without a browser, use
`sn profile add --no-verify` with the profile name and OAuth flags. That only saves the
configuration; it doesn't sign you in. You still need to complete `sn profile login`
with a browser that can reach the CLI's callback.

### Manage the session

After login, tokens refresh transparently. Manage the session with `sn profile status` (method + token expiry), `sn profile refresh`, and `sn profile logout`. The client ID and redirect URI live in `config.toml`; the secret and tokens in `credentials.toml` (both files are `0600`).

Use `--profile NAME` to select a profile for these commands, or run `sn ping` to check
the default profile's connection.

## Configuration files

Credentials use a two-file, AWS CLI-style split:

| File | Contains | Location (Linux) |
|---|---|---|
| `config.toml` | Instance URLs, default profile, non-secret OAuth config | `~/.config/sn/` |
| `credentials.toml` | Usernames, passwords, secrets, cached tokens | `~/.config/sn/` |
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

For proxy and CA settings, precedence is: CLI flag > environment variable > profile config.

TLS verification is disabled if **any** of `--insecure`, `SN_INSECURE=1`, or the profile's
`insecure = true` enables it. There is no per-command flag to override an insecure
profile; rewrite the profile to re-enable verification, and clear any environment override.

Passing `--proxy`, `--insecure`, `--ca-cert`, or `--proxy-ca-cert` to `sn init` or
`sn profile add` saves it in the profile. `sn profile show` and `sn profile list` report
what's stored: `insecure` always, plus `proxy`, `no_proxy`, `ca_cert`, and `proxy_ca_cert`
when set, with any password in the proxy URL masked. The two commands treat a flag you
leave out differently:

- **`sn profile add --force`** rebuilds these settings from the flags you pass, so
  re-adding without `--insecure` turns verification back on. It also drops a stored proxy
  unless you pass `--proxy` again. `no_proxy` has no flag and is kept.
- **`sn init`** updates in place: a flag you leave out keeps its stored value, and
  `--no-proxy` clears a stored proxy.
