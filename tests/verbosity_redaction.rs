//! `-ddd` must never print OAuth secrets. The transparent token refresh in
//! `build_client` posts to `/oauth_token.do`; the request side already logs only
//! form field names, and the response side must redact `access_token` /
//! `refresh_token` before the body reaches stderr. This drives a real refresh
//! through the compiled binary and asserts the freshly minted tokens do not
//! appear in the `-ddd` log, while the mask marker does.

mod common;

use common::{sn_cmd, write_oauth_profile};
use serde_json::json;
use sn::config::now_unix;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, ResponseTemplate};

const LEAKY_ACCESS_TOKEN: &str = "LEAKYACCESSTOKEN0xDEADBEEF";
const LEAKY_REFRESH_TOKEN: &str = "LEAKYREFRESHTOKEN0xFEEDFACE";

#[tokio::test(flavor = "current_thread")]
async fn ddd_does_not_leak_oauth_tokens() {
    let server = wiremock::MockServer::start().await;

    // Expired cached token + a refresh token → build_client refreshes silently.
    Mock::given(method("POST"))
        .and(path("/oauth_token.do"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": LEAKY_ACCESS_TOKEN,
            "refresh_token": LEAKY_REFRESH_TOKEN,
            "expires_in": 1800,
            "token_type": "Bearer",
            "scope": "useraccount"
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/now/table/incident/abc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "result": { "sys_id": "abc", "number": "INC0001" }
        })))
        .mount(&server)
        .await;

    let uri = server.uri();
    tokio::task::spawn_blocking(move || {
        // Cached token already expired (60s ago) so a refresh is forced.
        let tmp = write_oauth_profile("otest", &uri, "cid", now_unix() as i64 - 60);
        let mut cmd = sn_cmd(tmp.path());
        let out = cmd
            .args(["-ddd", "table", "get", "incident", "abc"])
            .assert()
            .success();
        let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();

        // The freshly minted secrets must not appear anywhere in the log...
        assert!(
            !stderr.contains(LEAKY_ACCESS_TOKEN),
            "access_token leaked to -ddd stderr:\n{stderr}"
        );
        assert!(
            !stderr.contains(LEAKY_REFRESH_TOKEN),
            "refresh_token leaked to -ddd stderr:\n{stderr}"
        );
        // ...but the token response WAS logged (redacted), and non-secret
        // metadata stays readable — proving redaction, not a dropped log line.
        assert!(
            stderr.contains("****"),
            "expected redaction marker in -ddd stderr:\n{stderr}"
        );
        assert!(
            stderr.contains("token_type"),
            "expected token response body to be logged:\n{stderr}"
        );
        drop(tmp);
    })
    .await
    .unwrap();
}
