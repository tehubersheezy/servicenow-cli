//! Signed JWT assertions for the OAuth `jwt_bearer` grant (RFC 7523).
//!
//! ServiceNow's inbound JWT endpoint ("Create an OAuth JWT API endpoint for
//! external clients") verifies a client-signed JWT against a certificate in a
//! JWT verifier map and trades it at `/oauth_token.do` for an access token. The
//! claims it checks: `aud` must equal the client_id, `iss` should (a mismatch
//! needs an extra claim validation on the instance), `sub` names the user via
//! the endpoint's User field, `exp` bounds it, and `jti` must be fresh on every
//! exchange while JTI verification is on (the default). The `kid` header picks
//! the verifier map entry.
//!
//! Signing uses `ring` directly rather than a JWT crate: `ring` is already in
//! the binary as rustls' crypto provider, and PEM parsing comes from
//! `rustls-pki-types`, also already linked — so this grant adds no crate to the
//! dependency tree. A JWT library would have brought its own RSA/ECDSA stack for
//! the ten lines of JOSE encoding below.

use crate::config::{self, JwtAlg, ResolvedJwt};
use crate::error::{Error, Result};
use base64::Engine;
use ring::rand::SystemRandom;
use ring::signature::{self, EcdsaKeyPair, RsaKeyPair};
use rustls_pki_types::PrivateKeyDer;
use rustls_pki_types::pem::PemObject;
use serde_json::json;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// How long a minted assertion is valid. It is used once, immediately, so this
/// only needs to cover the token request plus clock skew; ServiceNow's own
/// default skew allowance is 300s on top.
pub const ASSERTION_TTL_SECS: u64 = 300;

/// A private key loaded and bound to the algorithm it will sign with.
pub enum SigningKey {
    Rsa(RsaKeyPair, JwtAlg),
    Ecdsa(EcdsaKeyPair, JwtAlg),
}

impl std::fmt::Debug for SigningKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never print key material, not even through {:?}.
        write!(f, "SigningKey({})", self.alg().as_str())
    }
}

impl SigningKey {
    pub fn alg(&self) -> JwtAlg {
        match self {
            SigningKey::Rsa(_, a) | SigningKey::Ecdsa(_, a) => *a,
        }
    }

    fn sign(&self, msg: &[u8]) -> Result<Vec<u8>> {
        let rng = SystemRandom::new();
        match self {
            SigningKey::Rsa(kp, alg) => {
                let padding: &'static dyn signature::RsaEncoding = match alg {
                    JwtAlg::Rs384 => &signature::RSA_PKCS1_SHA384,
                    JwtAlg::Rs512 => &signature::RSA_PKCS1_SHA512,
                    _ => &signature::RSA_PKCS1_SHA256,
                };
                let mut sig = vec![0u8; kp.public().modulus_len()];
                kp.sign(padding, &rng, msg, &mut sig)
                    .map_err(|_| Error::Config("RSA signing failed".into()))?;
                Ok(sig)
            }
            // The FIXED encodings produce r||s, which is exactly JWS's format
            // (RFC 7518 §3.4) — no DER unwrapping needed.
            SigningKey::Ecdsa(kp, _) => kp
                .sign(&rng, msg)
                .map(|s| s.as_ref().to_vec())
                .map_err(|_| Error::Config("ECDSA signing failed".into())),
        }
    }
}

/// Parse a PEM private key and pick (or check) its signing algorithm.
///
/// Accepts PKCS#8 (`BEGIN PRIVATE KEY`, what `openssl genpkey` writes) for RSA
/// and EC, and PKCS#1 (`BEGIN RSA PRIVATE KEY`) for RSA. SEC1 EC keys
/// (`BEGIN EC PRIVATE KEY`) are refused with the one-line conversion, because
/// `ring` only loads EC keys from PKCS#8. Encrypted keys are refused likewise:
/// there is nobody to type a passphrase in an agent's invocation.
pub fn parse_key(pem: &[u8], alg: Option<JwtAlg>) -> Result<SigningKey> {
    let text = String::from_utf8_lossy(pem);
    if text.contains("ENCRYPTED PRIVATE KEY") {
        return Err(Error::Config(
            "the JWT key file is passphrase-encrypted; sn needs an unencrypted key \
             (openssl pkcs8 -topk8 -nocrypt -in <key> -out <new>) kept readable only by you"
                .into(),
        ));
    }
    let der = PrivateKeyDer::from_pem_slice(pem).map_err(|e| {
        Error::Config(format!(
            "the JWT key file is not a PEM private key ({e}); expected `-----BEGIN PRIVATE KEY-----`"
        ))
    })?;
    let rng = SystemRandom::new();
    let bad = |what: &str| Error::Config(format!("the JWT key file {what}"));
    match der {
        PrivateKeyDer::Sec1(_) => Err(bad(
            "is a SEC1 EC key, which cannot be loaded; convert it to PKCS#8 with \
             `openssl pkcs8 -topk8 -nocrypt -in <key> -out <new>`",
        )),
        PrivateKeyDer::Pkcs1(k) => {
            let kp = RsaKeyPair::from_der(k.secret_pkcs1_der())
                .map_err(|e| bad(&format!("holds an unusable RSA key: {e}")))?;
            rsa_with(kp, alg)
        }
        PrivateKeyDer::Pkcs8(k) => {
            let der = k.secret_pkcs8_der();
            if let Ok(kp) = RsaKeyPair::from_pkcs8(der) {
                return rsa_with(kp, alg);
            }
            let try_ec = |a: JwtAlg| {
                let spec = match a {
                    JwtAlg::Es384 => &signature::ECDSA_P384_SHA384_FIXED_SIGNING,
                    _ => &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
                };
                EcdsaKeyPair::from_pkcs8(spec, der, &rng)
                    .ok()
                    .map(|kp| SigningKey::Ecdsa(kp, a))
            };
            match alg {
                Some(a @ (JwtAlg::Es256 | JwtAlg::Es384)) => try_ec(a).ok_or_else(|| {
                    bad(&format!("does not hold a key usable with {}", a.as_str()))
                }),
                Some(a) => Err(bad(&format!(
                    "holds an EC key, which cannot sign {} (use ES256/ES384, or an RSA key)",
                    a.as_str()
                ))),
                None => try_ec(JwtAlg::Es256)
                    .or_else(|| try_ec(JwtAlg::Es384))
                    .ok_or_else(|| {
                        bad("holds a key type sn cannot sign with (supported: RSA ≥ 2048 bits, EC P-256, EC P-384)")
                    }),
            }
        }
        _ => Err(bad("holds an unsupported key encoding")),
    }
}

fn rsa_with(kp: RsaKeyPair, alg: Option<JwtAlg>) -> Result<SigningKey> {
    match alg.unwrap_or(JwtAlg::Rs256) {
        a @ (JwtAlg::Rs256 | JwtAlg::Rs384 | JwtAlg::Rs512) => Ok(SigningKey::Rsa(kp, a)),
        a => Err(Error::Config(format!(
            "the JWT key file holds an RSA key, which cannot sign {} (use RS256/RS384/RS512, or an EC key)",
            a.as_str()
        ))),
    }
}

/// Read and parse the key at `path`.
pub fn load_key(path: &str, alg: Option<JwtAlg>) -> Result<SigningKey> {
    let pem = std::fs::read(path)
        .map_err(|e| Error::Config(format!("cannot read JWT key file {path}: {e}")))?;
    parse_key(&pem, alg)
}

/// Build and sign one assertion for `client_id`, valid from `now` for
/// [`ASSERTION_TTL_SECS`]. `jti` is fresh randomness per call, so two
/// assertions are never byte-identical — ServiceNow rejects a replayed `jti`.
pub fn sign_assertion(
    key: &SigningKey,
    client_id: &str,
    subject: &str,
    kid: Option<&str>,
    now: u64,
) -> Result<String> {
    let mut header = json!({"alg": key.alg().as_str(), "typ": "JWT"});
    if let Some(k) = kid {
        header["kid"] = json!(k);
    }
    let mut jti = [0u8; 16];
    getrandom::getrandom(&mut jti).map_err(|e| Error::Transport(format!("rng failure: {e}")))?;
    let claims = json!({
        "iss": client_id,
        "sub": subject,
        "aud": client_id,
        "iat": now,
        "exp": now + ASSERTION_TTL_SECS,
        "jti": B64.encode(jti),
    });
    let signing_input = format!(
        "{}.{}",
        B64.encode(header.to_string()),
        B64.encode(claims.to_string())
    );
    let sig = key.sign(signing_input.as_bytes())?;
    Ok(format!("{signing_input}.{}", B64.encode(sig)))
}

/// The assertion for a resolved `jwt_bearer` profile: load its key, sign now.
pub fn assertion_for(client_id: &str, jwt: &ResolvedJwt) -> Result<String> {
    let key = load_key(&jwt.key_file, jwt.alg)?;
    sign_assertion(
        &key,
        client_id,
        &jwt.subject,
        jwt.kid.as_deref(),
        config::now_unix(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use ring::signature::KeyPair;

    fn pem(der: &[u8]) -> Vec<u8> {
        let b64 = base64::engine::general_purpose::STANDARD.encode(der);
        let body: Vec<String> = b64
            .as_bytes()
            .chunks(64)
            .map(|c| String::from_utf8(c.to_vec()).unwrap())
            .collect();
        format!(
            "-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----\n",
            body.join("\n")
        )
        .into_bytes()
    }

    fn ec_pkcs8(alg: &'static signature::EcdsaSigningAlgorithm) -> Vec<u8> {
        EcdsaKeyPair::generate_pkcs8(alg, &SystemRandom::new())
            .unwrap()
            .as_ref()
            .to_vec()
    }

    fn decode(part: &str) -> serde_json::Value {
        serde_json::from_slice(&B64.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn es256_assertion_has_the_servicenow_claims_and_verifies() {
        let der = ec_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING);
        let key = parse_key(&pem(&der), None).unwrap();
        assert_eq!(key.alg(), JwtAlg::Es256);
        let jwt = sign_assertion(&key, "cid", "svc.user", Some("k1"), 1_000).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3);

        let h = decode(parts[0]);
        assert_eq!(h["alg"], "ES256");
        assert_eq!(h["typ"], "JWT");
        assert_eq!(h["kid"], "k1");
        let c = decode(parts[1]);
        assert_eq!(c["iss"], "cid");
        assert_eq!(c["aud"], "cid");
        assert_eq!(c["sub"], "svc.user");
        assert_eq!(c["iat"], 1_000);
        assert_eq!(c["exp"], 1_000 + ASSERTION_TTL_SECS);
        assert!(c["jti"].as_str().is_some_and(|j| !j.is_empty()));

        let kp = EcdsaKeyPair::from_pkcs8(
            &signature::ECDSA_P256_SHA256_FIXED_SIGNING,
            &der,
            &SystemRandom::new(),
        )
        .unwrap();
        signature::UnparsedPublicKey::new(
            &signature::ECDSA_P256_SHA256_FIXED,
            kp.public_key().as_ref(),
        )
        .verify(
            format!("{}.{}", parts[0], parts[1]).as_bytes(),
            &B64.decode(parts[2]).unwrap(),
        )
        .expect("signature verifies");
    }

    #[test]
    fn p384_key_infers_es384() {
        let der = ec_pkcs8(&signature::ECDSA_P384_SHA384_FIXED_SIGNING);
        assert_eq!(parse_key(&pem(&der), None).unwrap().alg(), JwtAlg::Es384);
    }

    #[test]
    fn kid_is_omitted_when_unset_and_jti_is_fresh() {
        let der = ec_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING);
        let key = parse_key(&pem(&der), None).unwrap();
        let a = sign_assertion(&key, "cid", "u", None, 1).unwrap();
        let b = sign_assertion(&key, "cid", "u", None, 1).unwrap();
        assert!(decode(a.split('.').next().unwrap()).get("kid").is_none());
        let jti = |t: &str| decode(t.split('.').nth(1).unwrap())["jti"].clone();
        assert_ne!(
            jti(&a),
            jti(&b),
            "a replayed jti is rejected by the instance"
        );
    }

    #[test]
    fn ec_key_refuses_an_rsa_algorithm() {
        let der = ec_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING);
        let err = parse_key(&pem(&der), Some(JwtAlg::Rs256)).unwrap_err();
        assert!(err.to_string().contains("RS256"), "{err}");
    }

    #[test]
    fn garbage_and_sec1_and_encrypted_keys_are_config_errors() {
        assert!(matches!(
            parse_key(b"not a key", None),
            Err(Error::Config(_))
        ));
        let enc =
            b"-----BEGIN ENCRYPTED PRIVATE KEY-----\nAAAA\n-----END ENCRYPTED PRIVATE KEY-----\n";
        let e = parse_key(enc, None).unwrap_err().to_string();
        assert!(e.contains("-nocrypt"), "{e}");
        let sec1 = b"-----BEGIN EC PRIVATE KEY-----\nAAAA\n-----END EC PRIVATE KEY-----\n";
        let e = parse_key(sec1, None).unwrap_err().to_string();
        assert!(e.contains("pkcs8"), "{e}");
    }

    #[test]
    fn debug_never_prints_key_material() {
        let der = ec_pkcs8(&signature::ECDSA_P256_SHA256_FIXED_SIGNING);
        let key = parse_key(&pem(&der), None).unwrap();
        assert_eq!(format!("{key:?}"), "SigningKey(ES256)");
    }

    /// RSA keys can't be generated by `ring`, and committing a private key —
    /// even a throwaway — trips secret scanners, so this leans on `openssl`
    /// where the host has it (every CI runner does) and is a no-op otherwise.
    #[test]
    fn rsa_key_from_openssl_signs_rs256_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("k.pem");
        let ok = std::process::Command::new("openssl")
            .args([
                "genpkey",
                "-algorithm",
                "RSA",
                "-pkeyopt",
                "rsa_keygen_bits:2048",
                "-out",
            ])
            .arg(&path)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !ok {
            eprintln!("openssl unavailable; skipping RSA signing test");
            return;
        }
        let key = load_key(path.to_str().unwrap(), None).unwrap();
        assert_eq!(key.alg(), JwtAlg::Rs256);
        let SigningKey::Rsa(kp, _) = &key else {
            panic!("expected RSA")
        };
        let public = kp.public_key().as_ref().to_vec();
        let jwt = sign_assertion(&key, "cid", "u", None, 1).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        signature::UnparsedPublicKey::new(&signature::RSA_PKCS1_2048_8192_SHA256, public)
            .verify(
                format!("{}.{}", parts[0], parts[1]).as_bytes(),
                &B64.decode(parts[2]).unwrap(),
            )
            .expect("RS256 signature verifies");
        // An explicit RS512 on the same key is honored, not re-inferred.
        assert_eq!(
            load_key(path.to_str().unwrap(), Some(JwtAlg::Rs512))
                .unwrap()
                .alg(),
            JwtAlg::Rs512
        );
    }
}
