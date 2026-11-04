---
paths:
  - "src/client.rs"
  - "src/config.rs"
  - "src/amb.rs"
  - "tests/proxy_tls.rs"
  - "tests/env_var_isolation.rs"
---

# Proxy and TLS

For proxy and CA settings: CLI flag > env var > profile config file. `insecure` is an exception: flag, environment, and profile values are ORed, so remove every enabled setting to restore verification.

| CLI flag | Env var | config.toml field | Description |
|---|---|---|---|
| `--proxy <URL>` | `SN_PROXY` | `proxy` | HTTP/HTTPS/SOCKS5 proxy URL |
| `--no-proxy` | — | — | Bypass proxy for this invocation |
| — | `SN_NO_PROXY` | `no_proxy` | Comma-separated hosts to bypass |
| `--insecure` | `SN_INSECURE=1` | `insecure` | Disable TLS cert verification (off by default) |
| `--ca-cert <PATH>` | `SN_CA_CERT` | `ca_cert` | Custom CA cert for ServiceNow |
| `--proxy-ca-cert <PATH>` | `SN_PROXY_CA_CERT` | `proxy_ca_cert` | Custom CA cert for the proxy |

Proxy auth is stored per-profile in `credentials.toml` (`proxy_username`, `proxy_password`).
