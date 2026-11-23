# disctui patch

Source is the published diself 0.3.0 crate, SHA-256
`d6830c02959ed2d3342cad3ae3c7bd86ae8aec37606150bcbae6b18f81c2ed72`.

The HTTP client sends the supplied Authorization header. Browser identity,
analytics, and fetch-metadata headers are omitted. Redirects are disabled.
A gateway close with code 4004 ends the connection and its reconnect loop. Consecutive connection attempts remain bounded until READY or RESUMED. Handshakes have a deadline, and heartbeat intervals are validated.
Gateway Identify uses current property names. Unknown client-version metadata, the forced presence, and bot intents are omitted. Identify debug output redacts the token.

A session created in a browser keeps that browser's client properties. `ClientBuilder::with_client_properties` and `Gateway::connect` accept the properties object the creating client reported; Identify sends it unchanged, and the WebSocket handshake carries the web origin and that client's user agent. Without supplied properties the application identity (`disctui`) is sent and the handshake names no browser. In live runs Discord closed a web-created session with 4003 and revoked its token within 20 seconds of an Identify carrying other properties; with the creator's properties the session stayed open.
Guild subscriptions are sent by `Client` after READY through the public `Gateway::subscribe_to_guilds`, so a caller that drives `Gateway` directly decides when to subscribe.

The normalized manifest omits upstream examples and integration-test targets. In-source unit tests remain available.

The original MIT license is in LICENSE.
A channel type missing from `ChannelType` deserializes as `Unknown`. Previously one such channel (type 20 in live data) failed its whole guild in the channel and guild caches.
