use color_eyre::eyre::{Result, eyre};

const TOS_WARNING: &str = "\
WARNING: Using unofficial Discord clients violates Discord's Terms of Service.
Your account may be suspended or terminated. Use at your own risk.
See: https://support.discord.com/hc/en-us/articles/115002192352";

/// Retrieve the Discord user token.
///
/// Resolution order: `DISCTUI_TOKEN` env var -> system keyring -> error.
pub fn get_token() -> Result<String> {
    if let Some(token) = std::env::var("DISCTUI_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        tracing::info!("using token from DISCTUI_TOKEN env var");
        return Ok(token);
    }

    if let Ok(token) = get_saved_token() {
        tracing::info!("using token from system keyring");
        return Ok(token);
    }

    Err(eyre!(
        "no Discord token found. Set DISCTUI_TOKEN env var or store in system keyring.\n\n{TOS_WARNING}"
    ))
}

const KEYRING_SERVICE: &str = "disctui";
const TOKEN_ACCOUNT: &str = "discord_token";
const CLIENT_PROPERTIES_ACCOUNT: &str = "discord_client_properties";

fn keyring_entry(account: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(KEYRING_SERVICE, account)
        .map_err(|e| eyre!("failed to create keyring entry: {e}"))
}

/// Read the message session saved by interactive sign-in.
pub fn get_saved_token() -> Result<String> {
    let entry = keyring_entry(TOKEN_ACCOUNT)?;
    let token = entry
        .get_password()
        .map_err(|e| eyre!("failed to read saved message session: {e}"))?;
    if token.is_empty() {
        return Err(eyre!("saved message session is empty"));
    }
    Ok(token)
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

/// Store a token in the system keyring for future use.
///
/// Client properties saved for the previous session are removed, because
/// they describe the client that created that session and no other.
pub fn store_token(token: &str) -> Result<()> {
    set_secret(TOKEN_ACCOUNT, token)?;
    match keyring_entry(CLIENT_PROPERTIES_ACCOUNT)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(error) => {
            return Err(eyre!(
                "failed to remove the previous session's client properties: {error}"
            ));
        }
    }
    tracing::info!("token stored in system keyring");
    Ok(())
}

/// Save the properties of the client that created the session just stored.
///
/// Call this after [`store_token`] for a session created in the login
/// `WebView`. Discord revokes a session whose client changes, so the Gateway
/// connection presents these properties again.
#[cfg(target_os = "macos")]
pub fn store_client_properties(properties: &serde_json::Value) -> Result<()> {
    set_secret(CLIENT_PROPERTIES_ACCOUNT, &properties.to_string())
}

/// The saved client properties, when they belong to `token`.
///
/// They belong to the session in the keyring, so a token from anywhere else
/// (the `DISCTUI_TOKEN` variable, a pasted token) gets none.
pub fn client_properties_for(token: &str) -> Option<serde_json::Value> {
    if get_saved_token().ok()?.as_str() != token {
        return None;
    }
    let saved = match saved_client_properties() {
        Ok(saved) => saved?,
        Err(error) => {
            tracing::warn!("{error}");
            return None;
        }
    };
    match serde_json::from_str::<serde_json::Value>(&saved) {
        Ok(properties) if properties.is_object() => Some(properties),
        Ok(_) | Err(_) => {
            tracing::warn!("saved client properties are not a JSON object");
            None
        }
    }
}

fn saved_client_properties() -> Result<Option<String>> {
    match keyring_entry(CLIENT_PROPERTIES_ACCOUNT)?.get_password() {
        Ok(saved) => Ok(Some(saved)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(eyre!("failed to read saved client properties: {error}")),
    }
}

fn set_secret(account: &str, secret: &str) -> Result<()> {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use crate::auth::is_duplicate_item;

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
