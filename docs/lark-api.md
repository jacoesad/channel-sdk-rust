# Lark API

`lark-channel` is a Channel SDK, not a full Lark/Feishu OpenAPI SDK. The internal `lark_openapi` module contains only the Lark/Feishu OpenAPI calls needed by the current Channel workflow.

The selected domain comes from `ChannelConfig`:

- Feishu: `https://open.feishu.cn`
- Lark: `https://open.larksuite.com`

## Implemented APIs

| Official API | Method and path | SDK entry points |
| --- | --- | --- |
| App Access Token | `POST /open-apis/auth/v3/app_access_token/internal` | `OpenApiClient::app_access_token` |
| Tenant Access Token | `POST /open-apis/auth/v3/tenant_access_token/internal` | `OpenApiClient::tenant_access_token` |
| Create Message | `POST /open-apis/im/v1/messages` | `OpenApiClient::create_message` |
| Reply Message | `POST /open-apis/im/v1/messages/{message_id}/reply` | `OpenApiClient::reply_message` |
| Update Message Card | `PATCH /open-apis/im/v1/messages/{message_id}` | `OpenApiClient::update_message_card` |
| Create Card Entity | `POST /open-apis/cardkit/v1/cards` | `OpenApiClient::create_card_entity` |
| Full Update Card Entity | `PUT /open-apis/cardkit/v1/cards/{card_id}` | `OpenApiClient::update_card_entity` |
| WebSocket Endpoint | `POST /callback/ws/endpoint` | `OpenApiClient::websocket_endpoint` |
| Receive Message Event | WebSocket event `im.message.receive_v1` | `parse_lark_event_payload`, `ChannelEvent::parse_lark_payload` |
| Card Action Callback | WebSocket callback `card.action.trigger` | `parse_lark_event_payload`, `ChannelEvent::parse_lark_payload` |

Official docs:

- [App Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/app_access_token_internal.md)
- [Tenant Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/tenant_access_token_internal.md)
- [Create Message](https://open.feishu.cn/document/server-docs/im-v1/message/create.md)
- [Message Content](https://open.feishu.cn/document/server-docs/im-v1/message-content-description/create_json.md)
- [Reply Message](https://open.feishu.cn/document/server-docs/im-v1/message/reply.md)
- [Update Message Card](https://open.feishu.cn/document/server-docs/im-v1/message-card/patch.md)
- [Create Card Entity](https://open.feishu.cn/document/cardkit-v1/card/create.md)
- [Full Update Card Entity](https://open.feishu.cn/document/cardkit-v1/card/update.md)
- [Receive Message](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive.md)
- [Card Action Callback](https://open.feishu.cn/document/feishu-cards/card-callback-communication.md)
- [Use long connections to receive events](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case.md)

## WebSocket Endpoint Mapping

`OpenApiClient::websocket_endpoint` maps to the long-connection endpoint used by the official SDK family:

- request path: `POST /callback/ws/endpoint`
- request body: `AppID` and `AppSecret`
- response `data.URL` becomes `WebSocketEndpoint::url`
- response `data.ClientConfig` becomes `WebSocketClientConfig`

The endpoint URL is validated as `ws` or `wss` and must include the `device_id` and `service_id` query fields used by the long-connection protocol.

With the optional `websocket` feature enabled, `TokioTungsteniteWebSocketTransport` can connect to the endpoint and read/write raw `WebSocketFrame` values.

Event data frames can be parsed with `WebSocketFrame::event` or received with `WebSocketConnection::next_event`. `next_event` moves the payload bytes into `WebSocketEvent` and returns a lightweight `WebSocketEventFrame` for ACK metadata, so callers do not need to keep a second copy of large event payloads. The event envelope exposes the protocol headers needed by the official long-connection flow:

- `message_id`
- `trace_id`
- `sum`
- `seq`
- raw payload bytes

`WebSocketConnection::next_event` is a lower-level raw-frame API and returns
one Lark/Feishu protocol packet at a time. Higher-level Channel helpers
`EventConsumer` and `EventLoop` reassemble application-level split packets whose
protocol metadata reports `sum > 1` before parsing payloads or invoking
handlers. This is separate from WebSocket protocol fragmentation handled by the
underlying WebSocket implementation: Lark/Feishu split packets are identified by
the `sum` and `seq` headers carried inside each event data frame.

Packet sequence values are zero-based for multi-packet events. The reassembler
groups packets by `message_id` and `trace_id`, stores only received packet
bytes, and joins packets in `seq` order once all parts are present. Duplicate
packet sequences replace earlier bytes. The default high-level limits are 1024
parts, 16 MiB per logical event, and 128 pending logical events; callers can tune
them with `EventPacketReassemblyOptions` on `EventConsumer` or `EventLoop`.

`WebSocketFrame::event_ack_frame`, `WebSocketEventFrame::event_ack_frame`, and `WebSocketConnection::ack_event` build and send the ACK frame for a handled event. The ACK payload follows the official SDK shape:

- success: `{"code":200}`
- failure: `{"code":500}`
- optional `data` is a caller-provided base64 string
- optional `biz_rt` is sent as the `biz_rt` frame header

The higher-level `EventConsumer` wraps a single `WebSocketConnection` and combines receive, Lark event parsing, handler execution, and ACK sending. `handle_next_event` adds a `biz_rt` ACK header when the handler returns an ACK without one. If event parsing fails after a WebSocket event frame has been received, `EventConsumer` attempts to send an internal-server-error ACK before returning. Handler errors are also ACKed as internal-server-error before the original handler error is returned to the caller.

`EventLoop` uses the same receive, parse, handler, and ACK semantics with an `EventStreamConnector` to keep consuming events across clean closes and transport errors. The built-in `OpenApiWebSocketEventConnector` requests a fresh WebSocket endpoint before each connection attempt. If the effective reconnect limit is reached after clean closes, the loop returns `EventLoopExit::ReconnectLimitReached`; if the final retryable failure is a transport error, the loop returns that error. By default, the loop follows endpoint `ClientConfig` values for reconnect policy: `ReconnectCount=-1` means unlimited retries, non-negative `ReconnectCount` values are finite retry counts, and `ReconnectInterval`/`ReconnectNonce` are used only when they are positive durations. Explicit local reconnect options disable server-provided reconnect policy unless `with_server_reconnect_config(true)` is used.

WebSocket ping frames are answered by `WebSocketConnection`, and the event loop sends the official application-level heartbeat ping at the endpoint-provided `PingInterval` while waiting for the next event. If the endpoint omits a positive `PingInterval`, the connection falls back to 120 seconds. Heartbeat send failures and optional heartbeat liveness timeouts during receive waits are treated as reconnectable transport errors. Internally, `EventLoop` now separates the receive/reassembly path, handler dispatch/ACK policy, and protocol write facade, while still executing them over one mutable connection. User handlers are awaited without driving connection heartbeats; handlers should return promptly or spawn long-running work outside the event loop. A future runtime should turn these internal boundaries into independently driven receive, heartbeat, reconnect, dispatcher, and serialized writer tasks so handler work and connection heartbeats can run concurrently.

`WebSocketFrame::heartbeat_ping` builds the official heartbeat control frame (`method=Control`, `type=ping`, `SeqID=0`, `LogID=0`, and the endpoint `service_id`). `WebSocketConnection` preserves the endpoint `ClientConfig`, exposes the positive `PingInterval` as a `Duration`, and applies `ClientConfig` updates carried by application-level `pong` control frames. Application-level control frames are surfaced to the event loop as activity so heartbeat liveness checks can be cleared without waiting for a data event.

## Event Mapping

`parse_lark_event_payload` maps the official Lark/Feishu event and callback envelope into `ChannelEvent`:

- `im.message.receive_v1` -> `ChannelEvent::Message`
- `card.action.trigger` -> `ChannelEvent::CardAction`
- other event types -> `ChannelEvent::Unknown`

For received messages, resource descriptors are derived from the official received-message content shapes documented in [接收消息内容结构](https://open.feishu.cn/document/server-docs/im-v1/message-content-description/message_content): top-level `image`, `file`, `folder`, `audio`, `media`, and `sticker` messages, plus embedded `img` and `media` elements inside rich-text `post` content.

`ChannelEvent::CardAction` preserves the full raw callback payload and exposes the bridge-critical card interaction fields:

- `context`: event id, tenant key, and create time from the callback header
- `operator`: tenant key, user id, open id, and union id when provided
- `token`: the short-lived token used by future card update helpers
- `action`: component tag, name, timezone, developer-provided `value`, form/input/select values, checked state, and the raw action object
- `host`, `delivery_type`, and card display context such as `open_message_id` and `open_chat_id`

## Message Mapping

`OpenApiClient::create_message` maps SDK message types to the official create-message API:

- `Recipient::Chat(chat_id)` -> `receive_id_type=chat_id`, `receive_id=<chat_id>`
- `Recipient::User(open_id)` -> `receive_id_type=open_id`, `receive_id=<open_id>`
- `MessageContent::Text` -> `msg_type=text`
- `MessageContent::Post` -> `msg_type=post`
- `MessageContent::Card` -> `msg_type=interactive`
- `MessageContent::CardReference` -> `msg_type=interactive` with a CardKit `card_id` reference
- `MessageContent::Custom` -> caller-provided `msg_type`
- `content` is serialized as the JSON string required by the official API
- `uuid` comes from `MessageCreateOptions`

`OpenApiClient::reply_message` maps SDK message types to the official reply-message API:

- `MessageId` -> path field `{message_id}`
- `MessageContent::Text` -> `msg_type=text`
- `MessageContent::Post` -> `msg_type=post`
- `MessageContent::Card` -> `msg_type=interactive`
- `MessageContent::CardReference` -> `msg_type=interactive` with a CardKit `card_id` reference
- `MessageContent::Custom` -> caller-provided `msg_type`
- `content` is serialized as the JSON string required by the official API
- `uuid` and `reply_in_thread` come from `MessageReplyOptions`

## Card Mapping

`Card` represents validated CardKit JSON 2.0. `CardBuilder` emits `schema=2.0`, a shared-card `config.update_multi=true` setting, and common header/body components. `CardElement::raw` and `Card::from_value` preserve access to official CardKit fields and components that are not modeled by the convenience builder.

Two update identities are intentionally distinct:

- `OpenApiClient::update_message_card` targets the `message_id` returned after sending an inline card. The official endpoint requires `config.update_multi=true` on the card before and after the update.
- `OpenApiClient::create_card_entity` returns a `CardId`. Send it with `MessageContent::CardReference` or the high-level `card_reference_message`/`card_reference_reply` helpers, then use `OpenApiClient::update_card_entity` for full replacements.

CardKit entity updates require a strictly increasing positive `sequence` for every operation on the same card. `CardUpdateOptions` validates the documented `1..=2147483647` range and optional 64-character `uuid`, but sequence persistence and cross-task synchronization remain caller responsibilities. Card entities are valid for 14 days and can be sent once.

Element-level CardKit updates, streaming-mode configuration, and callback-token response updates remain later Milestone 5 work.

## Error Handling

The OpenAPI response parser currently handles:

- non-2xx HTTP status as `Error::HttpStatus { status }`
- `code != 0` as `Error::Api { code, message }`
- JSON decoding failures as `Error::Serde`

Both `msg` and `message` are accepted as API error message aliases because official APIs and observed responses may use either field name.

## Not Yet Exposed

The current subset intentionally does not expose:

- `receive_id_type=union_id`, `user_id`, or `email`
- user-token based message create/reply
- full response message models beyond `data.message_id`
- automatic event dispatch beyond the current `EventConsumer` and `EventLoop` handlers
- a complete Lark/Feishu OpenAPI surface
