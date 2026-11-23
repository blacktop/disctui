use super::embed::{Embed, EmbedAuthor, EmbedField, EmbedFooter, EmbedImage, EmbedThumbnail};

/// Fluent builder for creating Discord embeds.
///
/// # Example
/// ```
/// use diself::model::EmbedBuilder;
///
/// let embed = EmbedBuilder::new()
///     .title("Hello")
///     .description("World")
///     .color(0xFF0000)
///     .field("Name", "Value", true)
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct EmbedBuilder {
    title: Option<String>,
    description: Option<String>,
    url: Option<String>,
    timestamp: Option<String>,
    color: Option<u32>,
    footer_text: Option<String>,
    footer_icon_url: Option<String>,
    image_url: Option<String>,
    thumbnail_url: Option<String>,
    author_name: Option<String>,
    author_url: Option<String>,
    author_icon_url: Option<String>,
    fields: Vec<EmbedField>,
}

impl EmbedBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn url(mut self, url: impl Into<String>) -> Self {
        self.url = Some(url.into());
        self
    }

    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    pub fn color(mut self, color: u32) -> Self {
        self.color = Some(color);
        self
    }

    pub fn footer(mut self, text: impl Into<String>, icon_url: Option<String>) -> Self {
        self.footer_text = Some(text.into());
        self.footer_icon_url = icon_url;
        self
    }

    pub fn image(mut self, url: impl Into<String>) -> Self {
        self.image_url = Some(url.into());
        self
    }

    pub fn thumbnail(mut self, url: impl Into<String>) -> Self {
        self.thumbnail_url = Some(url.into());
        self
    }

    pub fn author(
        mut self,
        name: impl Into<String>,
        url: Option<String>,
        icon_url: Option<String>,
    ) -> Self {
        self.author_name = Some(name.into());
        self.author_url = url;
        self.author_icon_url = icon_url;
        self
    }

    pub fn field(
        mut self,
        name: impl Into<String>,
        value: impl Into<String>,
        inline: bool,
    ) -> Self {
        self.fields.push(EmbedField {
            name: name.into(),
            value: value.into(),
            inline,
        });
        self
    }

    pub fn build(self) -> Embed {
        Embed {
            title: self.title,
            kind: "rich".to_string(),
            description: self.description,
            url: self.url,
            timestamp: self.timestamp,
            color: self.color,
            footer: self.footer_text.map(|text| EmbedFooter {
                text,
                icon_url: self.footer_icon_url,
                proxy_icon_url: None,
            }),
            image: self.image_url.map(|url| EmbedImage {
                name: String::new(),
                url,
            }),
            thumbnail: self.thumbnail_url.map(|url| EmbedThumbnail {
                url,
                proxy_url: None,
                height: None,
                width: None,
            }),
            video: None,
            provider: None,
            author: self.author_name.map(|name| EmbedAuthor {
                name,
                url: self.author_url,
                icon_url: self.author_icon_url,
                proxy_icon_url: None,
            }),
            fields: self.fields,
        }
    }
}

impl From<EmbedBuilder> for Embed {
    fn from(builder: EmbedBuilder) -> Self {
        builder.build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embed_builder_basic() {
        let embed = EmbedBuilder::new()
            .title("Test")
            .description("Description")
            .color(0xFF0000)
            .build();

        assert_eq!(embed.title.as_deref(), Some("Test"));
        assert_eq!(embed.description.as_deref(), Some("Description"));
        assert_eq!(embed.color, Some(0xFF0000));
        assert_eq!(embed.kind, "rich");
    }

    #[test]
    fn test_embed_builder_fields() {
        let embed = EmbedBuilder::new()
            .field("F1", "V1", true)
            .field("F2", "V2", false)
            .build();

        assert_eq!(embed.fields.len(), 2);
        assert_eq!(embed.fields[0].name, "F1");
        assert!(embed.fields[0].inline);
        assert!(!embed.fields[1].inline);
    }

    #[test]
    fn test_embed_builder_serializes() {
        let embed = EmbedBuilder::new()
            .title("Hello")
            .description("World")
            .build();
        let json = serde_json::to_value(&embed).unwrap();
        assert_eq!(json["title"], "Hello");
        assert_eq!(json["description"], "World");
    }

    #[test]
    fn test_embed_from_builder() {
        let embed: Embed = EmbedBuilder::new().title("From").into();
        assert_eq!(embed.title.as_deref(), Some("From"));
    }
}
