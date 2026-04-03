#[cfg(feature = "experimental-discord")]
use color_eyre::eyre::{Result, eyre};

#[cfg(feature = "experimental-discord")]
const TOS_WARNING: &str = "\
WARNING: Using unofficial Discord clients violates Discord's Terms of Service.
Your account may be suspended or terminated. Use at your own risk.
See: https://support.discord.com/hc/en-us/articles/115002192352";

/// Retrieve the Discord user token.
///
/// Resolution order: `DISCTUI_TOKEN` env var -> system keyring -> error.
#[cfg(feature = "experimental-discord")]
pub fn get_token() -> Result<String> {
    if let Some(token) = std::env::var("DISCTUI_TOKEN")
        .ok()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
    {
        tracing::info!("using token from DISCTUI_TOKEN env var");
        return Ok(token);
    }

    match keyring::Entry::new("disctui", "discord_token") {
        Ok(entry) => match entry.get_password() {
            Ok(token) if !token.is_empty() => {
                tracing::info!("using token from system keyring");
                return Ok(token);
            }
            _ => {}
        },
        Err(e) => {
            tracing::warn!("keyring access failed: {e}");
        }
    }

    Err(eyre!(
        "no Discord token found. Set DISCTUI_TOKEN env var or store in system keyring.\n\n{TOS_WARNING}"
    ))
}

/// Store a token in the system keyring for future use.
#[cfg(feature = "experimental-discord")]
pub fn store_token(token: &str) -> Result<()> {
    let entry = keyring::Entry::new("disctui", "discord_token")
        .map_err(|e| eyre!("failed to create keyring entry: {e}"))?;
    entry
        .set_password(token)
        .map_err(|e| eyre!("failed to store token in keyring: {e}"))?;
    tracing::info!("token stored in system keyring");
    Ok(())
}

/// Print the terms of service warning to the log.
#[cfg(feature = "experimental-discord")]
pub fn log_tos_warning() {
    tracing::warn!("{TOS_WARNING}");
}
