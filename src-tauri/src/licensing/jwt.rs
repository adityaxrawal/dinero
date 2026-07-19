use anyhow::{Context, Result};
use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};

/// Public key embedded at build time (Doc 22 §10.2, §10.3: `include_str!("keys/license_public.pem")`).
///
/// ⚠️ See `keys/README.md` — this is currently a placeholder keypair with no
/// real Licensing Backend counterpart and must be replaced before release.
const EMBEDDED_PUBLIC_KEY_PEM: &str = include_str!("../../keys/license_public.pem");

/// Claims carried by the Licensing Backend's RSA-256 JWT (Doc 22 §10.3, Doc 19 §14.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicenseClaims {
    pub sub: String,
    pub device_id: String,
    pub plan: String,
    pub billing_interval: String,
    pub exp: i64,
}

/// Verifies a license JWT's RSA-256 signature and expiry against the embedded
/// public key (Doc 22 §10.3). This is the cryptographic check every
/// paid-feature-gated command must perform before trusting license state —
/// `subscription_status_cached` is display-only and must never be trusted alone.
pub fn verify_license_jwt(jwt: &str) -> Result<LicenseClaims> {
    verify_license_jwt_with_key(jwt, EMBEDDED_PUBLIC_KEY_PEM)
}

/// Core verification logic, parameterized on the public key PEM so it can be
/// exercised in tests against a dedicated test keypair — the embedded
/// placeholder has no real matching private key to sign test tokens with.
fn verify_license_jwt_with_key(jwt: &str, public_key_pem: &str) -> Result<LicenseClaims> {
    let decoding_key = DecodingKey::from_rsa_pem(public_key_pem.as_bytes())
        .context("Failed to parse RSA public key")?;

    let mut validation = Validation::new(Algorithm::RS256);
    validation.validate_exp = true;

    let token_data = decode::<LicenseClaims>(jwt, &decoding_key, &validation)
        .context("License JWT signature verification failed")?;

    Ok(token_data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use jsonwebtoken::{encode, EncodingKey, Header};

    // Dedicated test-only RSA-2048 keypair, generated once for this test suite.
    // Unrelated to the embedded production placeholder in keys/license_public.pem
    // and never used outside this test module.
    const TEST_PRIVATE_KEY_PEM: &str = "-----BEGIN PRIVATE KEY-----
MIIEvgIBADANBgkqhkiG9w0BAQEFAASCBKgwggSkAgEAAoIBAQC7uXV4lFFmcvzN
A7y3Z9NWxIzYCGY777EQT3jBgmdkuJZ8L+oEhh33FsHn/a5JsWQuBDiFixU76c+S
8A3xLki2OTqY10AHCMrxYDfFoFXTm58Q0Q8xPgVWB4BXemM9+RBBDCyn8KgeZCtU
BSGE8jwKYkA7BN50U/gl6iKCXg6MmbHbI6kEHFlIs4siRPwAaIdG38Jtl21OCivn
/NBE9Mq2a8kNDRVvOET0xLHSvmv6ZZL//M71wiuKXqxbkmc2s0HyKUruT+Vd/GuJ
hP0ESsaCMYXVAlvND7KfcIRZykxJIiQEOm+sATH7BY2vx62XH2IfNVK4YEqBKkMo
MflDm6I3AgMBAAECggEAVYFdA2YzSYHYqh0oqTVuatgt+vygbG557R71ttaJ97Oo
P7qnUhYwseo4uk2vRDu7kMY1ZIZ8ToTqGlijURau4elhSlrI/CtCHP2hia/FSBRb
OJKw49IHJi9WgwHpEEJQ5//+myQfv3AK2ENaCC85r9UewoMuPDg/EC38N/tVjy5u
I0LCjLpzSisIpFcYkoyGr0KN54wvxiQRIqgFZT6kY79G2nPt0iSoXR+Pw2STDdP/
Zqn0Igi4hF7NgpQADTZKfsSk9EN4OP8SVXLemvS1c67qgO+Q4gXnFmc3fCo83/I8
kuGDOWYCyzeSNL7pavyNgIRAqgf5QCakqeX7ugA6+QKBgQDtP+TdO0I9cUZ5F21O
6zuypvaYXSY5cruRoNBtL/+YSm4zEVg+UAtGguBwncK55Yr9ybgPhygWCKsqynmO
j3TON8cMNntN/Yq5dnXtHlHkoCa84WGmI/YsX7DZYnCkTO2pK50jkHK27Q81PFAU
IOhP9x/87ntlBIyskC5kKzV+6wKBgQDKj46juORmU6MTINeMV8ZhauAOJhHlQWei
JzFB+fx2wpN0ILahXgQbrp5PeBhyZtkLucKcHdFirAvfnZ9CjjvN+wlXOpewgj92
rFFGwF9lPuP6L/V1Xs7r5ZiQBHKykyqEzUJZQ4179i2YuZMKgH9290wH5B0kveNL
g/dsyeDO5QKBgQDE77HJ1sPgfuPx5nls9iUC4kd2KHRvYYbDlssMp7gyMS6So4Yt
i4IgkMk/kiUu7JTYoBZyhU3IJH/5MEOBDCH4gCJxR9RI4/rAgs3W+8Ec17fwN+I3
6EgTg4com+dG6zioobR57hDbOaLTHPKYEszkfA2IhmnVa9Zd8/0eVyO76wKBgQCk
00vuTXUNDuGTnxXGTWZPvveyi6fkNORvPhUG6rRUrt/tjvENPcN6Aw0u/TpnXOkg
cXe6MUlAUV8YBtqg/bwMUEm5gSQNrO6XUoCQYdk+OX6pBn1llFAsBBewmO38AWSH
y75BhIaMaDWSIO9VjFosI+7qPOS6EQnzWN7s1xjnQQKBgCl5wF5GNuH6F1ns6KkU
2mcyklflN/RNX6cwBsOCW/x3NPhx6jvOyAmqGOXO68lPolYIKz47MfkKgmDIZlQo
HYGJsTwc6Q8bg9jtuMXxLl73xfXck+fjVUK7EW9zrjSrDx69QyU6UGB8Az3XpxyD
/UFz/NANlqZ1zFmrA/2CHTeg
-----END PRIVATE KEY-----
";
    const TEST_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAu7l1eJRRZnL8zQO8t2fT
VsSM2AhmO++xEE94wYJnZLiWfC/qBIYd9xbB5/2uSbFkLgQ4hYsVO+nPkvAN8S5I
tjk6mNdABwjK8WA3xaBV05ufENEPMT4FVgeAV3pjPfkQQQwsp/CoHmQrVAUhhPI8
CmJAOwTedFP4Jeoigl4OjJmx2yOpBBxZSLOLIkT8AGiHRt/CbZdtTgor5/zQRPTK
tmvJDQ0VbzhE9MSx0r5r+mWS//zO9cIril6sW5JnNrNB8ilK7k/lXfxriYT9BErG
gjGF1QJbzQ+yn3CEWcpMSSIkBDpvrAEx+wWNr8etlx9iHzVSuGBKgSpDKDH5Q5ui
NwIDAQAB
-----END PUBLIC KEY-----
";

    fn sign_test_jwt(claims: &LicenseClaims) -> String {
        let encoding_key = EncodingKey::from_rsa_pem(TEST_PRIVATE_KEY_PEM.as_bytes()).unwrap();
        encode(&Header::new(Algorithm::RS256), claims, &encoding_key).unwrap()
    }

    fn valid_claims() -> LicenseClaims {
        LicenseClaims {
            sub: "user@example.com".into(),
            device_id: "device-abc".into(),
            plan: "full".into(),
            billing_interval: "monthly".into(),
            exp: chrono::Utc::now().timestamp() + 3600,
        }
    }

    #[test]
    fn test_valid_signature_and_claims_verify() {
        let claims = valid_claims();
        let jwt = sign_test_jwt(&claims);
        let verified = verify_license_jwt_with_key(&jwt, TEST_PUBLIC_KEY_PEM).unwrap();
        assert_eq!(verified.sub, "user@example.com");
        assert_eq!(verified.device_id, "device-abc");
        assert_eq!(verified.plan, "full");
    }

    #[test]
    fn test_expired_jwt_rejected() {
        let mut claims = valid_claims();
        claims.exp = chrono::Utc::now().timestamp() - 3600;
        let jwt = sign_test_jwt(&claims);
        let result = verify_license_jwt_with_key(&jwt, TEST_PUBLIC_KEY_PEM);
        assert!(result.is_err(), "Expired JWT must be rejected");
    }

    #[test]
    fn test_signature_from_wrong_key_rejected() {
        let claims = valid_claims();
        let jwt = sign_test_jwt(&claims);
        // Verifying a JWT signed by the test private key against the *embedded
        // placeholder* public key (an unrelated keypair) must fail — this proves
        // signature verification is real, not a no-op that just decodes claims.
        let result = verify_license_jwt_with_key(&jwt, EMBEDDED_PUBLIC_KEY_PEM);
        assert!(
            result.is_err(),
            "JWT signed by a different key must not verify"
        );
    }

    #[test]
    fn test_tampered_payload_rejected() {
        let claims = valid_claims();
        let jwt = sign_test_jwt(&claims);
        let mut parts: Vec<&str> = jwt.split('.').collect();
        // Corrupt one character of the base64url payload segment.
        let mut payload = parts[1].to_string();
        let last = payload.pop().unwrap();
        payload.push(if last == 'A' { 'B' } else { 'A' });
        parts[1] = &payload;
        let tampered = parts.join(".");
        let result = verify_license_jwt_with_key(&tampered, TEST_PUBLIC_KEY_PEM);
        assert!(
            result.is_err(),
            "Tampered JWT payload must fail signature verification"
        );
    }

    #[test]
    fn test_embedded_placeholder_key_is_well_formed() {
        // Sanity check: the embedded placeholder key must at least be valid PEM,
        // even though nothing will ever legitimately validate against it.
        let result = DecodingKey::from_rsa_pem(EMBEDDED_PUBLIC_KEY_PEM.as_bytes());
        assert!(
            result.is_ok(),
            "Embedded placeholder public key must be valid PEM"
        );
    }
}
