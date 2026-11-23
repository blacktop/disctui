use super::reaction::Emoji;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poll {
    ///The question of the poll.
    pub question: String,

    /// Answers options for the poll.
    pub answers: Vec<PollAnswerOption>,

    /// The time when the poll ends, in ISO8601 format.
    pub expiry: String,

    /// Whether the poll allow mutiple answers or not.
    #[serde(default)]
    pub allow_multiselect: bool,

    /// Layout type (only 0 for default layout currently)
    pub layout_type: u8,

    /// The results of the poll
    #[serde(default)]
    pub results: Vec<PollResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollAnswerOption {
    /// The answer unique ID
    pub answer_id: String,
    /// Poll Media Object
    pub media: Option<PollMedia>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollMedia {
    emoji: Option<Emoji>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollResult {
    /// Whether the votes have been precisely counted
    #[serde(default)]
    pub is_finalized: bool,

    /// The counts for each answer
    #[serde(default)]
    pub answer_counts: Vec<PollAnswerCount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollAnswerCount {
    /// The answer unique ID
    pub id: String,

    /// The number of votes for this answer
    pub count: u64,

    /// Whether the current user has voted for this answer
    #[serde(default)]
    pub me_voted: bool,
}
