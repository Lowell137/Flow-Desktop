//! A signed-in YouTube session, kept in the OS keychain and never in a file.
//!
//! shortcut: cookies YouTube rotates later are not saved back, so a session ends when
//! the stored ones expire; store `Set-Cookie` values if sign-ins turn out short-lived.

use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use sha1::{Digest, Sha1};

use crate::security::keychain::{CredentialError, CredentialStore, Keychain};

const KEYCHAIN: Keychain = Keychain::new("io.github.aedev.flow.desktop.youtube");
const KEYCHAIN_ENTRY: &str = "session";

pub struct Session {
    cookie_header: String,
    sapisid: String,
}

impl Session {
    /// None until the cookies include the ones only a signed-in session has.
    pub fn from_cookies<'a>(cookies: impl IntoIterator<Item = (&'a str, &'a str)>) -> Option<Self> {
        let mut header = Vec::new();
        let mut sapisid = None;
        let mut fallback = None;
        for (name, value) in cookies {
            match name {
                "SAPISID" => sapisid = Some(value),
                "__Secure-3PAPISID" => fallback = Some(value),
                _ => {}
            }
            header.push(format!("{name}={value}"));
        }
        Some(Self {
            sapisid: sapisid.or(fallback)?.to_string(),
            cookie_header: header.join("; "),
        })
    }

    /// A Netscape cookies file. Only `youtube.com` cookies are kept, so nothing else from the
    /// file is ever sent.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        Self::from_cookies(text.lines().filter_map(|line| {
            let line = line.strip_prefix("#HttpOnly_").unwrap_or(line);
            let fields: Vec<&str> = line.split('\t').collect();
            let [domain, _, _, _, _, name, value] = fields[..] else {
                return None;
            };
            let domain = domain.trim_start_matches('.');
            (!line.starts_with('#') && (domain == "youtube.com" || domain.ends_with(".youtube.com")))
                .then_some((name, value))
        }))
    }

    fn encode(&self) -> String {
        format!("{}\n{}", self.sapisid, self.cookie_header)
    }

    fn decode(text: &str) -> Option<Self> {
        let (sapisid, cookie_header) = text.split_once('\n')?;
        Some(Self {
            sapisid: sapisid.to_string(),
            cookie_header: cookie_header.to_string(),
        })
    }

    #[must_use]
    pub fn cookie_header(&self) -> &str {
        &self.cookie_header
    }

    #[must_use]
    pub fn authorization(&self, origin: &str) -> String {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_secs());
        sapisid_hash(&self.sapisid, origin, now)
    }
}

fn sapisid_hash(sapisid: &str, origin: &str, now: u64) -> String {
    let digest = Sha1::digest(format!("{now} {sapisid} {origin}"));
    let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    format!("SAPISIDHASH {now}_{hex}")
}

/// The session for this run, read once; signing in or out restarts the app. A keychain that
/// cannot be read means signed out, and what it holds is never logged.
pub fn session() -> Option<&'static Session> {
    static SESSION: OnceLock<Option<Session>> = OnceLock::new();
    SESSION
        .get_or_init(|| match KEYCHAIN.read(KEYCHAIN_ENTRY) {
            Ok(stored) => stored.as_deref().and_then(Session::decode),
            Err(error) => {
                tracing::warn!(%error, "Could not read the YouTube session");
                None
            }
        })
        .as_ref()
}

/// Saves `session`, and answers Ok only once the keychain gives the same bytes back, since a
/// caller may delete the only other copy.
pub fn store_session(session: &Session) -> Result<(), CredentialError> {
    write_verified(&KEYCHAIN, session)
}

fn write_verified(store: &impl CredentialStore, session: &Session) -> Result<(), CredentialError> {
    let encoded = session.encode();
    store.write(KEYCHAIN_ENTRY, &encoded)?;
    if store.read(KEYCHAIN_ENTRY)?.as_deref() == Some(encoded.as_str()) {
        Ok(())
    } else {
        Err(CredentialError("The system keychain did not keep the session"))
    }
}

pub fn clear_session() -> Result<(), CredentialError> {
    KEYCHAIN.remove(KEYCHAIN_ENTRY)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_only_youtube_cookies() {
        let session = Session::parse(
            "# Netscape HTTP Cookie File\n\
             .youtube.com\tTRUE\t/\tTRUE\t0\tSAPISID\tabc\n\
             #HttpOnly_.youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdef\n\
             .google.com\tTRUE\t/\tTRUE\t0\tNID\tleak\n\
             .notyoutube.com\tTRUE\t/\tTRUE\t0\tBAD\tleak\n",
        )
        .unwrap();
        assert_eq!(session.cookie_header(), "SAPISID=abc; SID=def");
    }

    #[test]
    fn a_file_without_sapisid_is_signed_out() {
        assert!(Session::parse(".youtube.com\tTRUE\t/\tTRUE\t0\tSID\tdef\n").is_none());
    }

    #[test]
    fn hash_matches_the_web_format() {
        assert_eq!(
            sapisid_hash("abc", "https://www.youtube.com", 1),
            "SAPISIDHASH 1_76945995212dc850298a9feccea18a02639c6f64"
        );
    }

    #[test]
    fn a_stored_session_reads_back_the_same() {
        let session = Session::from_cookies([("SAPISID", "abc"), ("SID", "def")]).unwrap();
        let read = Session::decode(&session.encode()).unwrap();
        assert_eq!(read.cookie_header(), "SAPISID=abc; SID=def");
        assert_eq!(read.authorization("o")[..11], session.authorization("o")[..11]);
    }

    #[test]
    fn cookies_without_a_signed_in_one_are_no_session() {
        assert!(Session::from_cookies([("VISITOR_INFO1_LIVE", "x")]).is_none());
    }

    #[test]
    #[ignore = "writes a fake entry to this machine's real keychain"]
    fn the_real_keychain_keeps_a_session() {
        let store = Keychain::new("io.github.aedev.flow.desktop.keychain-test");
        let session = Session::from_cookies([("SAPISID", "fake")]).unwrap();
        write_verified(&store, &session).unwrap();
        store.remove(KEYCHAIN_ENTRY).unwrap();
        assert_eq!(store.read(KEYCHAIN_ENTRY).unwrap(), None);
    }
}
