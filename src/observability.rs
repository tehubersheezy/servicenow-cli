use std::sync::atomic::{AtomicU8, Ordering};

static LEVEL: AtomicU8 = AtomicU8::new(0);

pub fn set_level(level: u8) {
    LEVEL.store(level, Ordering::SeqCst);
}

pub fn level() -> u8 {
    LEVEL.load(Ordering::SeqCst)
}

pub fn log_request(method: &str, url: &str) {
    if level() >= 1 {
        eprintln!("sn: {method} {url}");
    }
}

pub fn log_response(status: u16, elapsed_ms: u128) {
    if level() >= 1 {
        eprintln!("sn: -> {status} ({elapsed_ms}ms)");
    }
}

pub fn log_response_headers(headers: &reqwest::header::HeaderMap) {
    if level() >= 2 {
        for (k, v) in headers {
            let name = k.as_str();
            let value = header_display_value(name, v.to_str().unwrap_or("<bin>"));
            eprintln!("sn: < {name}: {value}");
        }
    }
}

/// Map a header (name, value) pair to the string that is safe to print in a
/// `-dd` log. Secret-bearing headers are masked to a generic `****`:
/// `authorization` carries the Basic/Bearer credential and `set-cookie` carries
/// the session token ServiceNow mints on login. All other headers pass through.
fn header_display_value(name: &str, value: &str) -> String {
    if name.eq_ignore_ascii_case("authorization") || name.eq_ignore_ascii_case("set-cookie") {
        "****".to_string()
    } else {
        value.to_string()
    }
}

pub fn log_body(direction: &str, body: &str) {
    if level() >= 3 {
        // Truncate on a char boundary: a raw byte-index slice panics mid-multi-byte UTF-8.
        let trimmed = if body.len() > 4096 {
            let end = (0..=4096)
                .rev()
                .find(|&i| body.is_char_boundary(i))
                .unwrap_or(0);
            &body[..end]
        } else {
            body
        };
        eprintln!("sn: {direction} body: {trimmed}");
    }
}

#[cfg(test)]
mod tests {
    use super::header_display_value;

    #[test]
    fn authorization_header_is_masked() {
        // Both Basic and Bearer credentials collapse to the same generic mask.
        assert_eq!(
            header_display_value("authorization", "Basic dXNlcjpwdw=="),
            "****"
        );
        assert_eq!(
            header_display_value("Authorization", "Bearer abc.def.ghi"),
            "****"
        );
    }

    #[test]
    fn set_cookie_header_is_masked() {
        // Case-insensitive: reqwest normalizes to lowercase, but be defensive.
        assert_eq!(
            header_display_value("set-cookie", "JSESSIONID=SECRET; Path=/"),
            "****"
        );
        assert_eq!(
            header_display_value("Set-Cookie", "glide_session_store=SECRET"),
            "****"
        );
    }

    #[test]
    fn ordinary_headers_pass_through() {
        assert_eq!(
            header_display_value("content-type", "application/json"),
            "application/json"
        );
        assert_eq!(header_display_value("x-transaction-id", "abc123"), "abc123");
    }
}
