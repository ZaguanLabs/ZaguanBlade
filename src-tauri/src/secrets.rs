//! Lightweight, dependency-free guards that keep credentials out of the Symbols
//! Index's LLM-facing output. The index stores references, not source — but
//! semantic-anchor `value`/`preview` strings are one of the few places raw file
//! content is echoed back (via `context_pack`). Two layers protect that surface:
//!
//! 1. [`is_secret_file`] — a deny-list of files whose *entire* content is a
//!    credential (private keys, keystores, cloud-credential/token files). Their
//!    contents are never indexed as semantic anchors. `.env` is deliberately
//!    NOT here: it is indexed for its variable *names*; its secret *values* are
//!    scrubbed by layer 2.
//! 2. [`redact_secret_tokens`] — scrubs high-entropy, credential-shaped tokens
//!    from an anchor value/preview before it reaches the model. Catches secrets
//!    embedded in otherwise-normal files (a hard-coded API key, a `.env` value)
//!    without touching routes, i18n keys, CSS tokens, or config keys.

/// Files whose whole content is key material / credentials and therefore have no
/// useful structural anchors. Matched case-insensitively against the file name.
const SECRET_FILE_EXTENSIONS: &[&str] = &[
    ".pem",
    ".key",
    ".p12",
    ".pfx",
    ".keystore",
    ".jks",
    ".ppk",
    ".asc",
    ".gpg",
];

/// Exact secret file names (SSH keys, package-manager auth, etc.).
const SECRET_FILE_NAMES: &[&str] = &[
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    ".netrc",
    ".npmrc",
    ".pypirc",
    ".pgpass",
    "credentials.json",
];

/// True when `path` names a file that is credentials/key-material end to end, so
/// it should never be scanned for semantic anchors. Conservative on purpose:
/// only unambiguous secret files, never source or `.env` (see module docs).
pub fn is_secret_file(path: &str) -> bool {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    let lower = name.to_ascii_lowercase();

    if SECRET_FILE_NAMES.iter().any(|n| lower == *n) {
        return true;
    }
    // `id_rsa.pub` and friends are public keys — do not deny those.
    if lower.ends_with(".pub") {
        return false;
    }
    SECRET_FILE_EXTENSIONS
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Credential prefixes that identify a token regardless of entropy (they can be
/// shorter or lower-entropy than the generic rule would catch). Case-sensitive:
/// these tokens are emitted verbatim by the tools that mint them.
const SECRET_PREFIXES: &[&str] = &[
    "AKIA",
    "ASIA",  // AWS access key id
    "AIza",  // Google API key
    "ya29.", // Google OAuth token
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_", // GitHub
    "glpat-",      // GitLab
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "xoxr-",
    "xoxs-", // Slack
    "sk_live_",
    "sk_test_",
    "ps_live_",
    "ps_test_",
    "pk_live_",
    "rk_live_",
    "sk-", // Stripe / OpenAI-style
    "shpat_",
    "shpss_",     // Shopify
    "eyJ",        // JWT (base64 of `{"`)
    "-----BEGIN", // PEM block header
];

const REDACTION: &str = "<redacted>";

/// Return `text` with any credential-looking tokens replaced by `<redacted>`.
/// Non-secret text (identifiers, routes, i18n keys, URLs, prose) is preserved
/// byte-for-byte.
pub fn redact_secret_tokens(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }
    let mut out = String::with_capacity(text.len());
    let mut token = String::new();
    for ch in text.chars() {
        if is_token_char(ch) {
            token.push(ch);
        } else {
            flush_token(&mut token, &mut out);
            out.push(ch);
        }
    }
    flush_token(&mut token, &mut out);
    out
}

fn flush_token(token: &mut String, out: &mut String) {
    if token.is_empty() {
        return;
    }
    if looks_secret(token) {
        out.push_str(REDACTION);
    } else {
        out.push_str(token);
    }
    token.clear();
}

/// Chars that stay *inside* a token. Structural punctuation (quotes, brackets,
/// `:`, `,`, whitespace, …) is a boundary, so paths, routes and URLs break into
/// short, non-secret pieces while contiguous keys stay whole.
fn is_token_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '/' | '_' | '-' | '+' | '=' | '.' | '@' | '~')
}

fn looks_secret(token: &str) -> bool {
    if token.len() < 8 {
        return false;
    }
    if SECRET_PREFIXES
        .iter()
        .any(|prefix| token.starts_with(prefix))
    {
        return true;
    }
    generic_high_entropy_secret(token)
}

/// A long, credential-shaped, high-entropy blob. Requires a mixed alnum body,
/// explicit base64 punctuation, or a long pure-hex string — so pure-alpha
/// identifiers (e.g. `AuthenticationProviderFactory`) and pure-digit values are
/// never mistaken for secrets.
fn generic_high_entropy_secret(token: &str) -> bool {
    if token.len() < 24 {
        return false;
    }
    if !token
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=' | '_' | '-'))
    {
        return false;
    }
    let has_digit = token.chars().any(|c| c.is_ascii_digit());
    let has_alpha = token.chars().any(|c| c.is_ascii_alphabetic());
    let has_base64_symbol = token.chars().any(|c| matches!(c, '+' | '/' | '='));
    let looks_hex = token.len() >= 32 && token.chars().all(|c| c.is_ascii_hexdigit());
    let credential_shape = (has_digit && has_alpha) || has_base64_symbol || looks_hex;
    if !credential_shape {
        return false;
    }
    shannon_bits_per_char(token) >= 3.5
}

fn shannon_bits_per_char(s: &str) -> f64 {
    let len = s.chars().count();
    if len == 0 {
        return 0.0;
    }
    let mut counts: std::collections::HashMap<char, u32> = std::collections::HashMap::new();
    for c in s.chars() {
        *counts.entry(c).or_insert(0) += 1;
    }
    let len_f = len as f64;
    -counts
        .values()
        .map(|&count| {
            let p = count as f64 / len_f;
            p * p.log2()
        })
        .sum::<f64>()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_files_are_denied() {
        for path in [
            "deploy/server.pem",
            "config/private.key",
            "certs/store.p12",
            "app.pfx",
            "release.keystore",
            "app.jks",
            "server.ppk",
            "backup.gpg",
            "/home/u/.ssh/id_rsa",
            "id_ed25519",
            "project/.npmrc",
            ".pypirc",
            "gcloud/credentials.json",
        ] {
            assert!(is_secret_file(path), "expected secret: {path}");
        }
    }

    #[test]
    fn non_secret_files_are_allowed() {
        for path in [
            "src/main.ts",
            "src/auth/credentials.ts", // code file, not a credential blob
            ".env",                    // indexed for variable names
            ".env.local",
            "config/app.yaml",
            "keys/id_rsa.pub", // public key
            "notes.md",
            "styles/theme.css",
        ] {
            assert!(!is_secret_file(path), "expected allowed: {path}");
        }
    }

    #[test]
    fn redacts_known_credential_prefixes() {
        for secret in [
            "AKIAIOSFODNN7EXAMPLE",
            "ghp_16CharsOfTokenABCDEFGHIJKLMNOP",
            "ps_live_EXAMPLE_synthetic_zaguan_key",
            "ps_test_EXAMPLE_synthetic_zaguan_key",
            "xoxb-2401-slack-token-value",
            "sk_live_EXAMPLE_synthetic_test_key",
            "AIzaSyD-ExampleGoogleApiKey1234567",
            "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.payload.sig",
        ] {
            let out = redact_secret_tokens(&format!("token = {secret}"));
            assert!(
                out.contains("<redacted>") && !out.contains(secret),
                "expected redaction of {secret}, got: {out}"
            );
        }
    }

    #[test]
    fn redacts_generic_high_entropy_blobs() {
        let hex40 = "0f1e2d3c4b5a69788796a5b4c3d2e1f00f1e2d3c"; // 40-char hex
        let mixed = "aB3xQ9zL2mN8pR4tV6wY0cE5dF7gH1jK"; // 32-char mixed alnum
        for secret in [hex40, mixed] {
            let out = redact_secret_tokens(&format!("value: {secret}"));
            assert!(out.contains("<redacted>"), "expected redaction, got: {out}");
            assert!(!out.contains(secret), "leaked {secret} in: {out}");
        }
    }

    #[test]
    fn preserves_legitimate_anchor_values() {
        // Routes, i18n keys, CSS tokens, identifiers, URLs, prose — none secret.
        for text in [
            "/api/v1/users/:id",
            "auth.login.button.submit",
            "--color-primary-500",
            "AuthenticationProviderFactory",
            "getUserAuthenticationToken",
            "https://api.example.com/v1/resource",
            "user@example.com",
            "The quick brown fox jumps over the lazy dog",
            "SELECT id, name FROM users WHERE active = 1",
            "1234567890",
        ] {
            assert_eq!(
                redact_secret_tokens(text),
                text,
                "should not have redacted: {text}"
            );
        }
    }

    #[test]
    fn preserves_surrounding_punctuation_and_structure() {
        let input = "apiKey=\"AKIAIOSFODNN7EXAMPLE\"; region=\"us-east-1\"";
        let out = redact_secret_tokens(input);
        assert!(out.starts_with("apiKey=\""));
        assert!(out.contains("<redacted>"));
        assert!(out.contains("region=\"us-east-1\"")); // untouched
    }

    #[test]
    fn empty_input_is_empty() {
        assert_eq!(redact_secret_tokens(""), "");
    }
}
