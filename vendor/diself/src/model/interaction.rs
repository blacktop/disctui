use serde::{Deserialize, Serialize};
use serde_repr::{Deserialize_repr, Serialize_repr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize_repr, Deserialize_repr)]
#[repr(u8)]
pub enum InteractionType {
    Ping = 1,
    ApplicationCommand = 2,
    MessageComponent = 3,
    ApplicationCommandAutocomplete = 4,
    ModalSubmit = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interaction {
    /// Unique ID of the interaction
    pub id: String,

    /// Type of the interaction
    #[serde(rename = "type")]
    pub kind: InteractionType,
    // TODO
}
