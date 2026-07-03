//! Integration tests for the OAuth loopback redirect server (`run_loopback`),
//! driven with real TCP sockets and no browser. Gated off Windows to mirror the
//! repo's caution around live socket/network tests on that platform.
#![cfg(not(target_os = "windows"))]

mod common;

use common::{free_port, send_loopback_request};
use std::thread;

/// The browser hits `/callback?code=…&state=…` with a matching state — the
/// server returns the authorization code.
#[test]
fn returns_code_on_valid_callback() {
    let port = free_port();
    let redirect_uri = format!("http://localhost:{port}/callback");
    let handle = thread::spawn(move || sn::oauth::run_loopback(&redirect_uri, "STATE123"));

    send_loopback_request(port, "/callback?code=CODE123&state=STATE123");

    let result = handle.join().unwrap();
    assert_eq!(result.unwrap(), "CODE123");
}

/// A callback whose `state` doesn't match the expected one is rejected as a
/// possible CSRF attempt.
#[test]
fn rejects_state_mismatch() {
    let port = free_port();
    let redirect_uri = format!("http://localhost:{port}/callback");
    let handle = thread::spawn(move || sn::oauth::run_loopback(&redirect_uri, "STATE123"));

    send_loopback_request(port, "/callback?code=CODE123&state=WRONG");

    let err = handle.join().unwrap().unwrap_err();
    assert!(matches!(err, sn::error::Error::Auth { .. }));
}

/// A stray `/favicon.ico` request (which browsers fire automatically) is 404'd
/// and ignored; the server keeps waiting and still resolves the real callback.
#[test]
fn ignores_favicon_then_returns_code() {
    let port = free_port();
    let redirect_uri = format!("http://localhost:{port}/callback");
    let handle = thread::spawn(move || sn::oauth::run_loopback(&redirect_uri, "STATE123"));

    send_loopback_request(port, "/favicon.ico");
    send_loopback_request(port, "/callback?code=CODE456&state=STATE123");

    assert_eq!(handle.join().unwrap().unwrap(), "CODE456");
}

/// An `?error=…` redirect (e.g. the user denied consent) surfaces as an error.
#[test]
fn surfaces_error_param() {
    let port = free_port();
    let redirect_uri = format!("http://localhost:{port}/callback");
    let handle = thread::spawn(move || sn::oauth::run_loopback(&redirect_uri, "STATE123"));

    send_loopback_request(port, "/callback?error=access_denied");

    let err = handle.join().unwrap().unwrap_err();
    assert!(matches!(err, sn::error::Error::Auth { .. }));
}
