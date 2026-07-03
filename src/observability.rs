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
            let value = if name.eq_ignore_ascii_case("authorization") {
                "Basic ****".to_string()
            } else {
                v.to_str().unwrap_or("<bin>").to_string()
            };
            eprintln!("sn: < {name}: {value}");
        }
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
