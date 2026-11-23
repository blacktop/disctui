mod client;

pub use client::HttpClient;

/// Discord API version
pub const API_VERSION: u8 = 10;

/// Discord API base URL
pub const BASE_URL: &str = "https://discord.com/api";

/// Helper to build Discord API URLs
///
/// # Example
/// ```
/// use diself::http;
///
/// let url = http::api_url("/channels/123456789/messages");
/// // Returns: "https://discord.com/api/v10/channels/123456789/messages"
/// ```
pub fn api_url(endpoint: &str) -> String {
    format!("{}/v{}{}", BASE_URL, API_VERSION, endpoint)
}
