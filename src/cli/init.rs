use crate::cli::GlobalFlags;
use crate::cli::profile::{
    Caller, ProfileAddArgs, SDK_CLIENT_ADVISORY, SavePolicy, resolve_input, resolve_name,
    save_and_verify, uses_sdk_client,
};
use crate::config::{AuthMethod, JwtAlg, OAuthGrant};
use crate::error::Result;

#[derive(clap::Args, Debug)]
pub struct InitArgs {
    /// Profile name to create or update (default: "default").
    #[arg(long)]
    pub profile: Option<String>,
    /// Instance: short name (`dev380385`) or full URL.
    #[arg(long)]
    pub instance: Option<String>,
    /// Authentication method: `basic` (username/password), `oauth` (SSO /
    /// OAuth 2.0), `apikey` (REST API key), or `token` (an externally issued
    /// bearer token, stored or fetched by --token-command).
    #[arg(long, value_enum)]
    pub auth: Option<AuthMethod>,
    /// Username (basic auth only).
    #[arg(long)]
    pub username: Option<String>,
    /// Password (basic auth only). Convenience flag; prefer the interactive
    /// prompt — `--password` is visible in `ps` output and shell history.
    #[arg(long)]
    pub password: Option<String>,
    /// REST API key (apikey auth only). Convenience flag; prefer the
    /// interactive prompt — `--api-key` is visible in `ps` output and shell
    /// history.
    #[arg(long)]
    pub api_key: Option<String>,
    /// Command whose stdout is the bearer token, run through the shell on
    /// demand (token auth only). Its output may be the bare token or JSON with
    /// `access_token` and `expires_in`/`expires_at`; the token is cached only
    /// when an expiry is given, otherwise the command runs on every call.
    #[arg(long, value_name = "COMMAND", conflicts_with = "token")]
    pub token_command: Option<String>,
    /// Static bearer token (token auth only). Convenience flag; prefer the
    /// interactive prompt or `--token-command` — `--token` is visible in `ps`
    /// output and shell history.
    #[arg(long)]
    pub token: Option<String>,
    /// OAuth client_id (oauth only). Defaults, for authorization_code, to the
    /// ServiceNow SDK's (now-sdk) public client 543e5655f77746a28228c6009a599dfb;
    /// registering your own OAuth client is highly advised.
    #[arg(long)]
    pub client_id: Option<String>,
    /// OAuth client secret (oauth confidential clients).
    #[arg(long)]
    pub client_secret: Option<String>,
    /// OAuth redirect URI (oauth only). Defaults to /sdk-oauth.do for the SDK
    /// client (paste the code back), else http://localhost:8400/callback.
    #[arg(long, value_name = "URL")]
    pub redirect_uri: Option<String>,
    /// OAuth grant: authorization_code (SSO, default), client_credentials, or
    /// jwt_bearer (sign a JWT with a local private key; no browser).
    #[arg(long, value_enum)]
    pub grant: Option<OAuthGrant>,
    /// PEM private key that signs the JWT assertion (jwt_bearer only). Its
    /// certificate must be in the instance's JWT verifier map. The path is
    /// stored; the key stays where it is.
    #[arg(long, value_name = "PATH")]
    pub jwt_key_file: Option<String>,
    /// JWT `sub` claim (jwt_bearer only): the user the token acts as, matched
    /// against the JWT endpoint's User field (e.g. a user_name or email).
    #[arg(long, value_name = "SUBJECT")]
    pub jwt_subject: Option<String>,
    /// JWT `kid` header naming the instance's verifier map entry (jwt_bearer only).
    #[arg(long, value_name = "KID")]
    pub jwt_kid: Option<String>,
    /// JWT signing algorithm (jwt_bearer only). Defaults to RS256 for an RSA
    /// key, ES256/ES384 for a P-256/P-384 key.
    #[arg(long, value_enum, ignore_case = true)]
    pub jwt_alg: Option<JwtAlg>,
    /// Disable PKCE for the authorization-code flow.
    #[arg(long)]
    pub no_pkce: bool,
}

/// `sn init` — the first-run wizard: stand up a profile and make it the one
/// commands use.
///
/// It shares its whole implementation with `sn profile add` (see
/// `cli::profile`), and differs only in the three policies that make it an
/// onboarding command rather than a scripting one: it **always** claims
/// `default_profile`, it upserts rather than refusing to overwrite, and it
/// always verifies. Use `sn profile add` to register an additional profile
/// without disturbing the default.
pub fn run(global: &GlobalFlags, args: InitArgs) -> Result<()> {
    let add = ProfileAddArgs {
        name: args.profile,
        instance: args.instance,
        auth: args.auth,
        username: args.username,
        password: args.password,
        password_stdin: false,
        api_key: args.api_key,
        api_key_stdin: false,
        token_command: args.token_command,
        token: args.token,
        token_stdin: false,
        client_id: args.client_id,
        client_secret: args.client_secret,
        client_secret_stdin: false,
        redirect_uri: args.redirect_uri,
        grant: args.grant,
        jwt_key_file: args.jwt_key_file,
        jwt_subject: args.jwt_subject,
        jwt_kid: args.jwt_kid,
        jwt_alg: args.jwt_alg,
        no_pkce: args.no_pkce,
        force: true,
        no_verify: false,
        set_default: true,
        non_interactive: false,
    };

    // Unlike `sn profile add`, a nameless `sn init` is the documented way to set
    // up the first profile, so the name falls back to "default".
    // `Caller::Init` is what keeps a missing-field error from naming
    // `--password-stdin` / `--non-interactive`: flags the shared core has but
    // `sn init` does not accept.
    let name = resolve_name(&add, Some("default".into()), Caller::Init)?;
    let input = resolve_input(&add, name, Caller::Init)?;
    // `init` is the onboarding wizard: it always claims the default profile,
    // always verifies, and upserts rather than refusing an existing name —
    // stored proxy/TLS settings included, which a flag it was not given leaves
    // alone.
    let user = save_and_verify(
        global,
        &input,
        SavePolicy {
            set_default: true,
            verify: true,
            refuse_existing: false,
            replace_connection: false,
        },
    )?;

    let (name, instance) = (&input.name, &input.instance);
    match input.auth {
        AuthMethod::Basic | AuthMethod::Apikey | AuthMethod::Token => {
            eprintln!("profile '{name}' saved and verified ({instance}).");
        }
        AuthMethod::Oauth => {
            let who = user.unwrap_or_else(|| "(unknown)".into());
            eprintln!(
                "profile '{name}' saved and authenticated via oauth ({instance}, user {who})."
            );
        }
    }
    eprintln!("'{name}' is now the default profile.");
    if uses_sdk_client(&input) {
        eprintln!("warning: {SDK_CLIENT_ADVISORY}.");
    }
    Ok(())
}
