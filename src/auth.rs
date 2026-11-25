use std::fs::File;

use color_eyre::eyre::{Result, WrapErr, bail, eyre};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use zeroize::{ZeroizeOnDrop, Zeroizing};

const TOS_WARNING: &str = "\
WARNING: Using unofficial Discord clients violates Discord's Terms of Service.
Your account may be suspended or terminated. Use at your own risk.
See: https://support.discord.com/hc/en-us/articles/115002192352";

const KEYRING_SERVICE: &str = "disctui";
const SESSION_ACCOUNT: &str = "discord_session";
const LEGACY_TOKEN_ACCOUNT: &str = "discord_token";
const LEGACY_PROPERTIES_ACCOUNT: &str = "discord_client_properties";
const SESSION_VERSION: u8 = 1;
const MAX_SESSION_BYTES: usize = 64 * 1024;

/// A message session and its optional client metadata, read and written together.
#[derive(Serialize, Deserialize, PartialEq, ZeroizeOnDrop)]
pub struct MessageSession {
    version: u8,
    token: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[zeroize(skip)]
    client_properties: Option<Value>,
}

impl MessageSession {
    pub fn new(token: String, client_properties: Option<Value>) -> Self {
        Self {
            version: SESSION_VERSION,
            token,
            client_properties,
        }
    }

    pub fn into_parts(mut self) -> (String, Option<Value>) {
        (
            std::mem::take(&mut self.token),
            self.client_properties.take(),
        )
    }

    fn validate(&self) -> Result<()> {
        if self.version != SESSION_VERSION {
            bail!("Unsupported saved message session version");
        }
        if self.token.trim().is_empty() {
            bail!("Saved message session is empty");
        }
        if self
            .client_properties
            .as_ref()
            .is_some_and(|properties| !properties.is_object())
        {
            bail!("Saved message session metadata is invalid");
        }
        Ok(())
    }
}

/// Prefer an explicitly configured token over the stored message session.
pub fn get_session() -> Result<MessageSession> {
    if let Ok(token) = std::env::var("DISCTUI_TOKEN") {
        let mut token = Zeroizing::new(token);
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            let token = if trimmed.len() == token.len() {
                std::mem::take(&mut *token)
            } else {
                trimmed.to_owned()
            };
            tracing::info!("using token from DISCTUI_TOKEN env var");
            return Ok(MessageSession::new(token, None));
        }
    }
    get_saved_session()
}

/// Read the saved record. Only legacy migration needs to read multiple items.
pub fn get_saved_session() -> Result<MessageSession> {
    if let Some(session) = read_session(&Keychain)? {
        return Ok(session);
    }
    let _lock = lock_session_store()?;
    migrate_legacy_session(&Keychain)
}

pub fn save_session(session: &MessageSession) -> Result<()> {
    let _lock = lock_session_store()?;
    write_session(&Keychain, session)?;
    verify_session(&Keychain, session)?;
    remove_legacy_items(&Keychain);
    Ok(())
}

fn lock_session_store() -> Result<File> {
    let dir = dirs::cache_dir()
        .ok_or_else(|| eyre!("No cache directory"))?
        .join("disctui");
    std::fs::create_dir_all(&dir)?;
    let lock = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(dir.join("message-session.lock"))?;
    lock.try_lock()
        .wrap_err("Could not lock the saved message session")?;
    Ok(lock)
}

trait SessionItems {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>>;
    fn write(&self, account: &str, secret: &str) -> Result<()>;
    fn remove(&self, account: &str) -> Result<()>;
}

struct Keychain;

impl SessionItems for Keychain {
    fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>> {
        match keyring_entry(account)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(eyre!("Failed to read Keychain item {account}: {error}")),
        }
    }

    fn write(&self, account: &str, secret: &str) -> Result<()> {
        let Err(error) = keyring_entry(account)?.set_password(secret) else {
            return Ok(());
        };
        #[cfg(target_os = "macos")]
        if is_duplicate_item(&error) {
            security_framework::passwords::set_generic_password(
                KEYRING_SERVICE,
                account,
                secret.as_bytes(),
            )
            .map_err(|error| eyre!("failed to update keyring item {account}: {error}"))?;
            tracing::info!("updated existing keyring item {account}");
            return Ok(());
        }
        Err(eyre!("failed to store keyring item {account}: {error}"))
    }

    fn remove(&self, account: &str) -> Result<()> {
        match keyring_entry(account)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

fn keyring_entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| eyre!("failed to create keyring entry: {e}"))
}

fn read_session(items: &impl SessionItems) -> Result<Option<MessageSession>> {
    let Some(value) = items.read(SESSION_ACCOUNT)? else {
        return Ok(None);
    };
    if value.len() > MAX_SESSION_BYTES {
        bail!("Saved message session is too large");
    }
    let session: MessageSession =
        serde_json::from_str(&value).map_err(|_| eyre!("Saved message session is invalid"))?;
    session.validate()?;
    Ok(Some(session))
}

fn write_session(items: &impl SessionItems, session: &MessageSession) -> Result<()> {
    session.validate()?;
    let value = Zeroizing::new(serde_json::to_string(session)?);
    if value.len() > MAX_SESSION_BYTES {
        bail!("Message session is too large to save");
    }
    items.write(SESSION_ACCOUNT, &value)
}

fn verify_session(items: &impl SessionItems, session: &MessageSession) -> Result<()> {
    if read_session(items)?.as_ref() != Some(session) {
        bail!("Keychain returned a different message session after saving");
    }
    Ok(())
}

fn migrate_legacy_session(items: &impl SessionItems) -> Result<MessageSession> {
    // Another disctui process may have saved the combined record before we locked.
    if let Some(session) = read_session(items)? {
        return Ok(session);
    }
    let mut token = items.read(LEGACY_TOKEN_ACCOUNT)?.ok_or_else(|| {
        eyre!("No Discord message session saved. Sign in to Discord.\n\n{TOS_WARNING}")
    })?;
    let properties = match items.read(LEGACY_PROPERTIES_ACCOUNT)? {
        Some(value) if value.len() <= MAX_SESSION_BYTES => Some(
            serde_json::from_str(&value)
                .map_err(|_| eyre!("Saved message session metadata is invalid"))?,
        ),
        Some(_) => bail!("Saved message session metadata is too large"),
        None => None,
    };
    let session = MessageSession::new(std::mem::take(&mut *token), properties);
    write_session(items, &session)?;
    // This account was absent under the lock, so failed verification can safely
    // remove the candidate and leave both legacy items available for a retry.
    let rollback = std::mem::DropGuard::new(items, |items| remove_item(items, SESSION_ACCOUNT));
    verify_session(items, &session)?;
    let _ = std::mem::DropGuard::dismiss(rollback);
    remove_legacy_items(items);
    Ok(session)
}

fn remove_legacy_items(items: &impl SessionItems) {
    for account in [LEGACY_TOKEN_ACCOUNT, LEGACY_PROPERTIES_ACCOUNT] {
        remove_item(items, account);
    }
}

fn remove_item(items: &impl SessionItems, account: &str) {
    if let Err(error) = items.remove(account) {
        tracing::warn!("Could not clean up Keychain item {account}: {error}");
    }
}

/// An HTTP client for requests that carry a credential. It follows no
/// redirects, so the credential never reaches a host other than the one named.
pub fn http_client(timeout: std::time::Duration) -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}

/// Retry one idempotent Discord GET when its connection fails before a response.
pub async fn get_with_retry(
    client: &reqwest::Client,
    token: &str,
    url: &str,
) -> Result<reqwest::Response> {
    let mut authorization = reqwest::header::HeaderValue::from_str(token)?;
    authorization.set_sensitive(true);
    let request = || {
        client
            .get(url)
            .header(reqwest::header::AUTHORIZATION, authorization.clone())
            .send()
    };
    match request().await {
        Err(error)
            if error.is_connect()
                || error.is_timeout()
                || error.is_request()
                || error.is_body() =>
        {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            Ok(request().await?)
        }
        result => Ok(result?),
    }
}

/// Print the terms of service warning to the log.
pub fn log_tos_warning() {
    tracing::warn!("{TOS_WARNING}");
}

#[cfg(target_os = "macos")]
fn is_duplicate_item(error: &keyring::Error) -> bool {
    let keyring::Error::PlatformFailure(error) = error else {
        return false;
    };
    error
        .downcast_ref::<security_framework::base::Error>()
        .is_some_and(|error| error.code() == -25299)
}

#[cfg(test)]
#[expect(
    clippy::unwrap_used,
    reason = "a failed fixture operation should fail the test"
)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    #[cfg(target_os = "macos")]
    use crate::auth::is_duplicate_item;
    use crate::auth::{
        LEGACY_PROPERTIES_ACCOUNT, LEGACY_TOKEN_ACCOUNT, MessageSession, SESSION_ACCOUNT,
        SessionItems, migrate_legacy_session, read_session, verify_session, write_session,
    };
    use color_eyre::eyre::{Result, bail};
    use zeroize::Zeroizing;

    #[derive(Default)]
    struct FakeKeychain {
        entries: RefCell<HashMap<String, String>>,
        reads: Cell<usize>,
        refuse_verification: bool,
    }

    impl FakeKeychain {
        fn legacy() -> Self {
            let keychain = Self::default();
            keychain
                .write(LEGACY_TOKEN_ACCOUNT, "example-session")
                .unwrap();
            keychain
                .write(LEGACY_PROPERTIES_ACCOUNT, r#"{"os":"Mac OS X"}"#)
                .unwrap();
            keychain
        }
    }

    impl SessionItems for FakeKeychain {
        fn read(&self, account: &str) -> Result<Option<Zeroizing<String>>> {
            self.reads.set(self.reads.get() + 1);
            let entries = self.entries.borrow();
            if self.refuse_verification
                && account == SESSION_ACCOUNT
                && entries.contains_key(account)
            {
                bail!("fixture denied readback");
            }
            Ok(entries.get(account).cloned().map(Zeroizing::new))
        }

        fn write(&self, account: &str, secret: &str) -> Result<()> {
            self.entries
                .borrow_mut()
                .insert(account.into(), secret.into());
            Ok(())
        }

        fn remove(&self, account: &str) -> Result<()> {
            self.entries.borrow_mut().remove(account);
            Ok(())
        }
    }

    #[test]
    fn migration_keeps_metadata_and_following_startup_reads_one_item() {
        let keychain = FakeKeychain::legacy();
        migrate_legacy_session(&keychain).unwrap();
        assert_eq!(keychain.entries.borrow().len(), 1);

        keychain.reads.set(0);
        let session = read_session(&keychain).unwrap().unwrap();
        assert_eq!(keychain.reads.get(), 1);
        assert_eq!(
            session.into_parts(),
            (
                "example-session".into(),
                Some(serde_json::json!({"os":"Mac OS X"}))
            )
        );
    }

    #[test]
    fn denied_migration_readback_leaves_legacy_items_available() {
        let mut keychain = FakeKeychain::legacy();
        keychain.refuse_verification = true;
        assert!(migrate_legacy_session(&keychain).is_err());
        let entries = keychain.entries.borrow();
        assert!(!entries.contains_key(SESSION_ACCOUNT));
        assert!(entries.contains_key(LEGACY_TOKEN_ACCOUNT));
        assert!(entries.contains_key(LEGACY_PROPERTIES_ACCOUNT));
    }

    #[test]
    fn replacing_a_saved_token_drops_the_old_metadata() {
        let keychain = FakeKeychain::legacy();
        migrate_legacy_session(&keychain).unwrap();
        let session = MessageSession::new("replacement".into(), None);
        write_session(&keychain, &session).unwrap();
        verify_session(&keychain, &session).unwrap();
        assert_eq!(
            read_session(&keychain).unwrap().unwrap().into_parts(),
            ("replacement".into(), None)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn modern_keychain_update_is_reserved_for_duplicate_items() {
        let platform_error = |code| {
            keyring::Error::PlatformFailure(Box::new(security_framework::base::Error::from_code(
                code,
            )))
        };
        assert!(is_duplicate_item(&platform_error(-25299)));
        assert!(!is_duplicate_item(&platform_error(-25293)));
        assert!(!is_duplicate_item(&keyring::Error::NoEntry));
    }
}
