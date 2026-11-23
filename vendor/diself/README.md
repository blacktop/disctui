# diself

A modern async Rust library for building Discord selfbot workflows with a clean API, typed models, and a resilient gateway runtime.

<p align="left">
  <a href="https://crates.io/crates/diself"><img alt="Crates.io" src="https://img.shields.io/crates/v/diself.svg" /></a>
  <a href="https://docs.rs/diself"><img alt="docs.rs" src="https://img.shields.io/docsrs/diself" /></a>
  <img alt="Rust" src="https://img.shields.io/badge/rust-2021-orange.svg" />
  <img alt="License" src="https://img.shields.io/badge/license-MIT-blue.svg" />
</p>

## Disclaimer

This project is intended for authorized and compliant use only.

- Make sure your usage complies with Discord's Terms of Service and platform policies.
- If you are using this for coursework, internal tooling, or controlled environments, ensure explicit authorization.
- Maintainers and contributors are not responsible for misuse.

## Features

- **Async-first** -- built on `tokio`, fully non-blocking
- **Event-driven** -- implement `EventHandler` and react to 60+ gateway events
- **Resilient gateway** -- automatic reconnect, resume, heartbeat ACK timeout, backoff with jitter, configurable max reconnect attempts
- **Typed models** -- channels, messages, guilds, roles, permissions, embeds, reactions, polls, ...
- **Configurable cache** -- users, channels, guilds, relationships (enable/disable per category)
- **Builder ergonomics** -- `ClientBuilder`, `EmbedBuilder`, `CreateMessage`
- **HTTP hardening** -- automatic rate-limit retry, configurable request delay, typed `Error::Api` variant
- **Graceful shutdown** -- `Client::shutdown()` for cooperative task cancellation

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
diself = "0.3.0"
tokio = { version = "1", features = ["full"] }
```

## Quick Start

```rust
use diself::prelude::*;

struct MyHandler;

#[async_trait]
impl EventHandler for MyHandler {
    async fn on_ready(&self, _ctx: &Context, user: User) {
        println!("Logged in as {}", user.tag());
    }

    async fn on_message_create(&self, ctx: &Context, msg: Message) {
        if msg.content == ".ping" {
            let _ = msg.reply(&ctx.http, "pong").await;
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let token = std::env::var("DISCORD_TOKEN").expect("DISCORD_TOKEN is not set");

    let client = Client::builder(token, MyHandler)?
        .with_cache_config(CacheConfig {
            cache_users: true,
            cache_channels: true,
            cache_guilds: true,
            cache_relationships: true,
        })
        .build();

    client.start().await
}
```

## Client Builder

`ClientBuilder` provides a clean, configurable entrypoint:

```rust
let client = Client::builder(token, handler)?
    .without_cache()
    .with_request_delay(Duration::from_millis(150))
    .with_captcha_handler(|captcha_info| async move {
        // Solve captcha and return captcha key
        Ok("captcha_key".to_string())
    })
    .build();
```

## Sending Messages

Simple text message:

```rust
ctx.send_message("channel_id", "Hello!").await?;
```

Rich message with embeds:

```rust
use diself::prelude::*;

let embed = EmbedBuilder::new()
    .title("Status")
    .description("All systems operational")
    .color(0x2ECC71)
    .field("Uptime", "99.9%", true)
    .field("Latency", "42ms", true)
    .footer("diself v0.3.0", None)
    .build();

let msg = CreateMessage::new()
    .content("Daily report")
    .embed(embed);

ctx.send_message_advanced("channel_id", msg).await?;
```

Reply to a message:

```rust
let reply = CreateMessage::new()
    .content("Got it!")
    .reply_to(&msg.id);

ctx.send_message_advanced(&msg.channel_id, reply).await?;
```

## Message History & Search

```rust
// Fetch last 50 messages
let messages = ctx.get_messages("channel_id", Some(50)).await?;

// Fetch with pagination
let older = ctx.channels.get_messages(
    &ctx.http, "channel_id",
    Some(100),               // limit
    Some(last_msg_id),       // before
    None,                    // after
    None,                    // around
).await?;

// Search messages in a guild
let results = ctx.guilds.search_messages(
    &ctx.http, "guild_id",
    &SearchParams { content: Some("hello".into()), ..Default::default() },
).await?;

// Pin / unpin
ctx.channels.pin_message(&ctx.http, "channel_id", "message_id").await?;
ctx.channels.unpin_message(&ctx.http, "channel_id", "message_id").await?;
let pinned = ctx.channels.get_pinned_messages(&ctx.http, "channel_id").await?;

// Purge own messages (deletes one by one, rate-limit aware)
let deleted = ctx.purge_own_messages("channel_id", 50).await?;
```

## Graceful Shutdown

Run the client in a task and stop it cooperatively:

```rust
use std::sync::Arc;

let client = Arc::new(Client::builder(token, handler)?.build());
let runner = Arc::clone(&client);

let task = tokio::spawn(async move {
    let _ = runner.start().await;
});

tokio::signal::ctrl_c().await?;
client.shutdown();
let _ = task.await;
```

## Gateway Reliability

The gateway runtime handles:

- Automatic reconnect with configurable max attempts (default 10)
- Session resume (`RESUME` opcode)
- Heartbeat with ACK timeout detection
- `RECONNECT` and `INVALID_SESSION` handling
- Exponential backoff with jitter

## Managers API

`Context` exposes endpoint managers for ergonomic calls:

| Manager | Access | Coverage |
|---------|--------|----------|
| Users | `ctx.users` | Profile, avatar, settings |
| Guilds | `ctx.guilds` | Members, roles, bans, search |
| Channels | `ctx.channels` | Messages, threads, pins, permissions |
| Relationships | `ctx.relationships` | Friends, blocks, requests |

```rust
let me = ctx.users.me(&ctx.http).await?;
let guilds = ctx.guilds.list(&ctx.http).await?;
let dms = ctx.channels.dm_channels(&ctx.http).await?;
```

## Examples

| Example | Description |
|---------|-------------|
| `examples/bot.rs` | Command handler with `!ping` and `!echo` |
| `examples/cache_example.rs` | Cache configuration and inspection |
| `examples/hello_gateway.rs` | Raw gateway connection test |

```bash
DISCORD_TOKEN="..." cargo run --example bot
```

## Development

```bash
cargo check
cargo clippy --all-targets --all-features
cargo test
```

Live endpoint smoke tests (requires a real token):

```bash
DISCORD_TOKEN="..." cargo test --test endpoints_live -- --ignored --nocapture
```

## Roadmap

Planned for upcoming releases:

- File upload / multipart in `CreateMessage` (0.3.1)
- Rate limit bucket tracking (pre/post request)
- Webhook management
- Voice gateway support
- Broader integration tests (gateway lifecycle, HTTP edge cases)

## Contributing

Contributions are welcome! Please read [CONTRIBUTING.md](CONTRIBUTING.md) before getting started.

## License

MIT. See [LICENSE](LICENSE).
