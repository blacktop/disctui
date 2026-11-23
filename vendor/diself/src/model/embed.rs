use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Embed {
    /// Title of embed
    #[serde(default)]
    pub title: Option<String>,

    /// Type of embed (rich, image, video, gifv, article, link, poll_result)
    #[serde(rename = "type")]
    pub kind: String,

    /// Description of embed
    #[serde(default)]
    pub description: Option<String>,

    /// URL of embed
    #[serde(default)]
    pub url: Option<String>,

    /// Timestamp of embed content
    #[serde(default)]
    pub timestamp: Option<String>,

    /// Color code of the embed
    #[serde(default)]
    pub color: Option<u32>,

    /// Footer information
    #[serde(default)]
    pub footer: Option<EmbedFooter>,

    /// Image information
    #[serde(default)]
    pub image: Option<EmbedImage>,

    /// Thumbnail information
    #[serde(default)]
    pub thumbnail: Option<EmbedThumbnail>,

    /// Video information
    #[serde(default)]
    pub video: Option<EmbedVideo>,

    /// Provider information
    #[serde(default)]
    pub provider: Option<EmbedProvider>,

    /// Author information
    #[serde(default)]
    pub author: Option<EmbedAuthor>,

    /// Fields information (max 25 fields)
    #[serde(default)]
    pub fields: Vec<EmbedField>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedFooter {
    /// Footer text
    pub text: String,

    /// URL of the footer icon (optional)
    #[serde(default)]
    pub icon_url: Option<String>,

    /// Proxy URL of the footer icon (optional)
    #[serde(default)]
    pub proxy_icon_url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedImage {
    /// Name of provider
    #[serde(default)]
    pub name: String,

    /// URL of provider
    #[serde(default)]
    pub url: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedThumbnail {
    /// URL of the thumbnail image
    pub url: String,

    /// Proxy URL of the thumbnail image (optional)
    #[serde(default)]
    pub proxy_url: Option<String>,

    /// Height of the thumbnail image (optional)
    #[serde(default)]
    pub height: Option<u64>,

    /// Width of the thumbnail image (optional)
    #[serde(default)]
    pub width: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedVideo {
    /// URL of the video
    #[serde(default)]
    pub url: String,

    /// Proxy URL of the video (optional)
    #[serde(default)]
    pub proxy_url: Option<String>,

    /// Height of the video
    #[serde(default)]
    pub height: u64,

    /// Width of the video
    #[serde(default)]
    pub width: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedProvider {
    /// Name of provider
    #[serde(default)]
    pub name: String,

    /// URL of provider
    #[serde(default)]
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedAuthor {
    /// Name of author
    pub name: String,

    /// URL of author
    #[serde(default)]
    pub url: Option<String>,

    /// Icon URL of author (optional)
    #[serde(default)]
    pub icon_url: Option<String>,

    /// Proxy URL of author icon (optional)
    #[serde(default)]
    pub proxy_icon_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedField {
    /// Name of the field
    pub name: String,

    /// Value of the field
    pub value: String,

    /// Whether the field should be displayed inline
    #[serde(default)]
    pub inline: bool,
}
