use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use argon2::password_hash::{PasswordHash, SaltString};
use argon2::{Argon2, PasswordHasher, PasswordVerifier};
use axum::http::HeaderMap;
use axum::http::header;

pub const COOKIE: &str = "bawkbit_sid";
pub const SESSION_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const MAX_FAILURES: u32 = 5;
const FAILURE_WINDOW: Duration = Duration::from_secs(10 * 60);
const BAN: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, PartialEq, Eq)]
pub enum LoginError {
    Blocked(u64),
    NoPassword,
    Wrong,
}

#[derive(Default)]
pub struct Auth {
    sessions: Mutex<HashMap<String, Instant>>,
    failures: Mutex<HashMap<IpAddr, (u32, Instant)>>,
}

impl Auth {
    pub fn login(
        &self,
        ip: IpAddr,
        web: &bawkbit_core::settings::Web,
        username: &str,
        password: &str,
    ) -> Result<String, LoginError> {
        let now = Instant::now();
        {
            let failures = self.failures.lock().unwrap();
            if let Some((count, at)) = failures.get(&ip)
                && *count >= MAX_FAILURES
                && now.duration_since(*at) < BAN
            {
                return Err(LoginError::Blocked(
                    (BAN - now.duration_since(*at)).as_secs() + 1,
                ));
            }
        }
        if web.password_hash.is_empty() {
            return Err(LoginError::NoPassword);
        }
        let name_ok = username.trim() == web.username.trim();
        let password_ok = verify_password(password, &web.password_hash);
        if !(name_ok && password_ok) {
            let mut failures = self.failures.lock().unwrap();
            let entry = failures.entry(ip).or_insert((0, now));
            if now.duration_since(entry.1) > FAILURE_WINDOW {
                *entry = (0, now);
            }
            entry.0 += 1;
            entry.1 = now;
            return Err(LoginError::Wrong);
        }
        self.failures.lock().unwrap().remove(&ip);
        let token = random_token();
        let mut sessions = self.sessions.lock().unwrap();
        sessions.retain(|_, expires| *expires > now);
        sessions.insert(token.clone(), now + SESSION_TTL);
        Ok(token)
    }

    pub fn check(&self, token: &str) -> bool {
        self.sessions
            .lock()
            .unwrap()
            .get(token)
            .is_some_and(|expires| *expires > Instant::now())
    }

    pub fn logout(&self, token: &str) {
        self.sessions.lock().unwrap().remove(token);
    }

    /// Ends every session, for when the password changes.
    pub fn clear(&self) {
        self.sessions.lock().unwrap().clear();
    }
}

pub fn hash_password(password: &str) -> anyhow::Result<String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|err| anyhow::anyhow!("no random source: {err}"))?;
    let salt = SaltString::encode_b64(&bytes)
        .map_err(|err| anyhow::anyhow!("cannot make a salt: {err}"))?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|err| anyhow::anyhow!("cannot hash the password: {err}"))
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// A password that is easy to read aloud: no 0/O or 1/l.
pub fn generate_password() -> String {
    const ALPHABET: &[u8] = b"abcdefghjkmnpqrstuvwxyz23456789";
    let mut bytes = [0u8; 14];
    let _ = getrandom::fill(&mut bytes);
    bytes
        .iter()
        .map(|byte| ALPHABET[*byte as usize % ALPHABET.len()] as char)
        .collect()
}

fn random_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS has a random source");
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn cookie_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .filter_map(|pair| pair.trim().split_once('='))
        .find(|(name, _)| *name == COOKIE)
        .map(|(_, value)| value.to_string())
}

pub fn session_cookie(token: &str) -> String {
    format!(
        "{COOKIE}={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        SESSION_TTL.as_secs()
    )
}

pub fn expired_cookie() -> String {
    format!("{COOKIE}=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

/// A request from this computer that no proxy forwarded; a local reverse proxy must not make the whole internet local.
pub fn is_local(addr: Option<SocketAddr>, headers: &HeaderMap) -> bool {
    let forwarded = ["forwarded", "x-forwarded-for", "x-real-ip"]
        .iter()
        .any(|name| headers.contains_key(*name));
    !forwarded && addr.is_some_and(|addr| addr.ip().is_loopback())
}

#[cfg(test)]
mod tests {
    use super::*;
    use bawkbit_core::settings::Web;

    fn web(password: &str) -> Web {
        Web {
            password_hash: hash_password(password).unwrap(),
            ..Web::default()
        }
    }

    #[test]
    fn hashes_verify() {
        let hash = hash_password("hunter22").unwrap();
        assert!(verify_password("hunter22", &hash));
        assert!(!verify_password("hunter23", &hash));
        assert!(!verify_password("hunter22", "not a hash"));
    }

    #[test]
    fn logs_in_and_out() {
        let auth = Auth::default();
        let ip: IpAddr = "10.0.0.2".parse().unwrap();
        let web = web("hunter22");
        let token = auth.login(ip, &web, "admin", "hunter22").unwrap();
        assert!(auth.check(&token));
        auth.logout(&token);
        assert!(!auth.check(&token));
        assert_eq!(
            auth.login(ip, &Web::default(), "admin", "x"),
            Err(LoginError::NoPassword)
        );
    }

    #[test]
    fn blocks_after_repeated_failures() {
        let auth = Auth::default();
        let ip: IpAddr = "10.0.0.3".parse().unwrap();
        let web = web("hunter22");
        for _ in 0..MAX_FAILURES {
            assert_eq!(
                auth.login(ip, &web, "admin", "wrong"),
                Err(LoginError::Wrong)
            );
        }
        assert!(matches!(
            auth.login(ip, &web, "admin", "hunter22"),
            Err(LoginError::Blocked(_))
        ));
    }

    #[test]
    fn reads_the_session_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(header::COOKIE, "a=1; bawkbit_sid=abc; b=2".parse().unwrap());
        assert_eq!(cookie_token(&headers).as_deref(), Some("abc"));
    }

    #[test]
    fn forwarded_requests_are_never_local() {
        let local: SocketAddr = "127.0.0.1:5000".parse().unwrap();
        let mut headers = HeaderMap::new();
        assert!(is_local(Some(local), &headers));
        headers.insert("x-forwarded-for", "8.8.8.8".parse().unwrap());
        assert!(!is_local(Some(local), &headers));
        assert!(!is_local(
            Some("10.1.1.1:1".parse().unwrap()),
            &HeaderMap::new()
        ));
    }
}
