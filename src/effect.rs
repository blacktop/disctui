use crate::model::MessageRow;

#[derive(Debug, Clone)]
#[expect(dead_code, reason = "fields read by effect executor via pattern match")]
pub enum Effect {
    LoadGuilds,
    LoadChannels {
        guild_id: String,
    },
    LoadHistory {
        channel_id: String,
    },
    SendMessage {
        channel_id: String,
        content: String,
    },
    SummarizeChannel {
        channel_id: String,
        channel_name: String,
        messages: Vec<MessageRow>,
        user_name: String,
    },
    FetchAvatar {
        url: String,
    },
}
