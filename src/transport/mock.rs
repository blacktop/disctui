use crate::model::{
    ChannelKind, ChannelSummary, DIRECT_MESSAGES_GUILD_ID, GuildSummary, MessageRow,
};

/// Load fixture guilds.
pub fn guilds() -> Vec<GuildSummary> {
    vec![
        GuildSummary {
            id: "g1".into(),
            name: "Rust Lang".into(),
            muted: false,
            unread: true,
            unread_count: 3,
            avatar_url: None,
        },
        GuildSummary {
            id: "g2".into(),
            name: "ratatui".into(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        },
        GuildSummary {
            id: "g3".into(),
            name: "Tokio".into(),
            muted: false,
            unread: true,
            unread_count: 12,
            avatar_url: None,
        },
        GuildSummary {
            id: "g4".into(),
            name: "Nix/NixOS".into(),
            muted: false,
            unread: false,
            unread_count: 0,
            avatar_url: None,
        },
    ]
}

/// Load fixture channels for a guild.
pub fn channels(guild_id: &str) -> Vec<ChannelSummary> {
    match guild_id {
        "g1" => vec![
            channel("c2", "g1", None, "announcements", false, 0, 0),
            category("cat1", "g1", "The Basics", 10),
            channel("c1", "g1", Some("cat1"), "general", true, 2, 11),
            channel("c3", "g1", Some("cat1"), "help", true, 1, 12),
            category("cat2", "g1", "Off Topic", 20),
            channel("c4", "g1", Some("cat2"), "off-topic", false, 0, 21),
            channel("c5", "g1", Some("cat2"), "showcase", false, 0, 22),
        ],
        "g2" => vec![
            category("cat10", "g2", "Community", 0),
            channel("c10", "g2", Some("cat10"), "general", false, 0, 1),
            category("cat11", "g2", "Development", 10),
            channel("c11", "g2", Some("cat11"), "development", false, 0, 11),
            channel("c12", "g2", Some("cat11"), "showcase", false, 0, 12),
        ],
        "g3" => vec![
            channel("c22", "g3", None, "announcements", false, 0, 0),
            category("cat20", "g3", "Async", 10),
            channel("c20", "g3", Some("cat20"), "general", true, 8, 11),
            channel("c21", "g3", Some("cat20"), "help", true, 4, 12),
        ],
        "g4" => vec![
            category("cat30", "g4", "Help", 0),
            channel("c30", "g4", Some("cat30"), "general", false, 0, 1),
            channel("c31", "g4", Some("cat30"), "nixos", false, 0, 2),
            category("cat31", "g4", "Tooling", 10),
            channel("c32", "g4", Some("cat31"), "flakes", false, 0, 11),
        ],
        DIRECT_MESSAGES_GUILD_ID => vec![
            direct_message("dm1", "alice", true, 2),
            direct_message("dm2", "build-bot", false, 0),
            direct_message("dm3", "incident-room", true, 1),
        ],
        _ => Vec::new(),
    }
}

/// Load fixture messages for a channel.
pub fn messages(channel_id: &str) -> Vec<MessageRow> {
    match channel_id {
        "c1" => rust_general_messages(),
        "c3" => rust_help_messages(),
        "c10" => ratatui_general_messages(),
        "c20" => tokio_general_messages(),
        "dm1" => direct_messages_from_alice(),
        "dm2" => build_bot_messages(),
        "dm3" => incident_room_messages(),
        _ => vec![MessageRow {
            id: format!("{channel_id}_m1"),
            channel_id: channel_id.into(),
            author: "system".into(),
            author_avatar_url: None,
            content: "Welcome to the channel!".into(),
            attachments: Vec::new(),
            timestamp: "09:00".into(),
            edited: false,
            is_continuation: false,
        }],
    }
}

fn direct_message(id: &str, name: &str, unread: bool, count: u32) -> ChannelSummary {
    ChannelSummary {
        id: id.into(),
        guild_id: Some(DIRECT_MESSAGES_GUILD_ID.into()),
        parent_id: None,
        name: name.into(),
        kind: ChannelKind::DirectMessage,
        position: 0,
        muted: false,
        unread,
        unread_count: count,
        last_message_id: None,
    }
}

fn category(id: &str, guild_id: &str, name: &str, position: i32) -> ChannelSummary {
    ChannelSummary {
        id: id.into(),
        guild_id: Some(guild_id.into()),
        parent_id: None,
        name: name.into(),
        kind: ChannelKind::Category,
        position,
        muted: false,
        unread: false,
        unread_count: 0,
        last_message_id: None,
    }
}

fn channel(
    id: &str,
    guild_id: &str,
    parent_id: Option<&str>,
    name: &str,
    unread: bool,
    count: u32,
    position: i32,
) -> ChannelSummary {
    ChannelSummary {
        id: id.into(),
        guild_id: Some(guild_id.into()),
        parent_id: parent_id.map(str::to_string),
        name: name.into(),
        kind: ChannelKind::Text,
        position,
        muted: false,
        unread,
        unread_count: count,
        last_message_id: None,
    }
}

fn msg(id: &str, ch: &str, author: &str, content: &str, time: &str, cont: bool) -> MessageRow {
    MessageRow {
        id: id.into(),
        channel_id: ch.into(),
        author: author.into(),
        author_avatar_url: None,
        content: content.into(),
        attachments: Vec::new(),
        timestamp: time.into(),
        edited: false,
        is_continuation: cont,
    }
}

fn rust_general_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "m1",
            "c1",
            "alice",
            "Has anyone tried the new borrow checker improvements in 1.82?",
            "10:15",
            false,
        ),
        msg(
            "m2",
            "c1",
            "bob",
            "Yeah, the diagnostics are so much better now",
            "10:17",
            false,
        ),
        msg(
            "m3",
            "c1",
            "bob",
            "Especially for closure captures",
            "10:17",
            true,
        ),
        msg(
            "m4",
            "c1",
            "charlie",
            "I'm still wrapping my head around async closures tbh",
            "10:20",
            false,
        ),
        msg(
            "m5",
            "c1",
            "alice",
            "The RFC for that just got merged! Should land in nightly soon",
            "10:22",
            false,
        ),
        msg(
            "m6",
            "c1",
            "dave",
            "Speaking of async, anyone using the new `async fn` in traits without dyn?",
            "10:25",
            false,
        ),
        msg(
            "m7",
            "c1",
            "alice",
            "We switched our whole codebase to it last week",
            "10:26",
            false,
        ),
        msg(
            "m8",
            "c1",
            "alice",
            "Huge improvement in ergonomics",
            "10:26",
            true,
        ),
        msg(
            "m9",
            "c1",
            "eve",
            "What about RPITIT? Is that stable yet?",
            "10:30",
            false,
        ),
        msg("m10", "c1", "bob", "Yes, since 1.75", "10:31", false),
        msg(
            "m11",
            "c1",
            "charlie",
            "The error messages for trait object safety got way better too",
            "10:33",
            false,
        ),
        msg(
            "m12",
            "c1",
            "dave",
            "@blacktop thoughts on the new edition?",
            "10:35",
            false,
        ),
        msg(
            "m13",
            "c1",
            "alice",
            "I think the edition migration tool handles most of it automatically",
            "10:37",
            false,
        ),
    ]
}

fn rust_help_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "h1",
            "c3",
            "newbie",
            "How do I convert a Vec<u8> to a String?",
            "11:00",
            false,
        ),
        msg(
            "h2",
            "c3",
            "helper",
            "Use String::from_utf8(vec) — it returns a Result since the bytes might not be valid UTF-8",
            "11:02",
            false,
        ),
        msg(
            "h3",
            "c3",
            "helper",
            "Or String::from_utf8_lossy(&vec) if you want to replace invalid bytes",
            "11:02",
            true,
        ),
        msg(
            "h4",
            "c3",
            "newbie",
            "Thanks! The lossy version is what I need",
            "11:03",
            false,
        ),
    ]
}

fn direct_messages_from_alice() -> Vec<MessageRow> {
    vec![
        msg(
            "dm1_m1",
            "dm1",
            "alice",
            "Can you review the latest Discord transport patch when you get a minute?",
            "08:45",
            false,
        ),
        msg(
            "dm1_m2",
            "dm1",
            "you",
            "Yes, send me the branch once you push it.",
            "08:47",
            false,
        ),
        msg(
            "dm1_m3",
            "dm1",
            "alice",
            "Pushed. The unread handling still looks a bit off in DMs.",
            "08:49",
            false,
        ),
    ]
}

fn build_bot_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "dm2_m1",
            "dm2",
            "build-bot",
            "nightly succeeded on main",
            "07:10",
            false,
        ),
        msg(
            "dm2_m2",
            "dm2",
            "build-bot",
            "coverage delta: +0.8%",
            "07:10",
            true,
        ),
    ]
}

fn incident_room_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "dm3_m1",
            "dm3",
            "ops",
            "Seeing rate limit spikes again. Can you check the gateway logs?",
            "09:12",
            false,
        ),
        msg(
            "dm3_m2",
            "dm3",
            "sre",
            "The reconnect loop looks clean, but DMs are still missing from the sidebar.",
            "09:13",
            false,
        ),
    ]
}

fn ratatui_general_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "r1",
            "c10",
            "orhun",
            "Just released ratatui 0.30! Check the changelog",
            "09:00",
            false,
        ),
        msg(
            "r2",
            "c10",
            "joshka",
            "The new Layout API is so clean",
            "09:05",
            false,
        ),
        msg(
            "r3",
            "c10",
            "tui_fan",
            "Anyone have examples of StatefulWidget patterns?",
            "09:10",
            false,
        ),
        msg(
            "r4",
            "c10",
            "orhun",
            "Check the examples/ directory, we added a bunch",
            "09:12",
            false,
        ),
        msg(
            "r5",
            "c10",
            "orhun",
            "Also the book has a whole chapter on it now",
            "09:12",
            true,
        ),
    ]
}

fn tokio_general_messages() -> Vec<MessageRow> {
    vec![
        msg(
            "t1",
            "c20",
            "carl",
            "Is there a way to gracefully shutdown a tokio runtime?",
            "14:00",
            false,
        ),
        msg(
            "t2",
            "c20",
            "sean",
            "Use tokio::signal::ctrl_c() and select! with your main loop",
            "14:02",
            false,
        ),
        msg(
            "t3",
            "c20",
            "carl",
            "What about tasks that are mid-flight?",
            "14:03",
            false,
        ),
        msg(
            "t4",
            "c20",
            "sean",
            "CancellationToken is the standard pattern",
            "14:05",
            false,
        ),
        msg(
            "t5",
            "c20",
            "sean",
            "Create one, pass clones to tasks, and cancel it on shutdown",
            "14:05",
            true,
        ),
        msg(
            "t6",
            "c20",
            "carl",
            "Perfect, that's exactly what I needed",
            "14:07",
            false,
        ),
        msg(
            "t7",
            "c20",
            "eliza",
            "Also look at tokio::task::JoinSet for managing groups of tasks",
            "14:10",
            false,
        ),
        msg(
            "t8",
            "c20",
            "eliza",
            "It has shutdown_all() which is really convenient",
            "14:10",
            true,
        ),
    ]
}
