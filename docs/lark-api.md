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
| WebSocket Endpoint | `POST /callback/ws/endpoint` | `OpenApiClient::websocket_endpoint` |
| Receive Message Event | WebSocket event `im.message.receive_v1` | `parse_lark_event_payload`, `ChannelEvent::parse_lark_payload` |
| Card Action Callback | WebSocket callback `card.action.trigger` | `parse_lark_event_payload`, `ChannelEvent::parse_lark_payload` |

Official docs:

- [App Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/app_access_token_internal.md)
- [Tenant Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/tenant_access_token_internal.md)
- [Create Message](https://open.feishu.cn/document/server-docs/im-v1/message/create.md)
- [Reply Message](https://open.feishu.cn/document/server-docs/im-v1/message/reply.md)
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

`WebSocketFrame::event_ack_frame`, `WebSocketEventFrame::event_ack_frame`, and `WebSocketConnection::ack_event` build and send the ACK frame for a handled event. The ACK payload follows the official SDK shape:

- success: `{"code":200}`
- failure: `{"code":500}`
- optional `data` is a caller-provided base64 string
- optional `biz_rt` is sent as the `biz_rt` frame header

The higher-level `EventConsumer` wraps a single `WebSocketConnection` and combines receive, Lark event parsing, handler execution, and ACK sending. `handle_next_event` adds a `biz_rt` ACK header when the handler returns an ACK without one. If event parsing fails after a WebSocket event frame has been received, `EventConsumer` attempts to send an internal-server-error ACK before returning. Handler errors are not ACKed so the platform can retry delivery.

`EventLoop` uses the same receive, parse, handler, and ACK semantics with an `EventStreamConnector` to keep consuming events across clean closes and transport errors. The built-in `OpenApiWebSocketEventConnector` requests a fresh WebSocket endpoint before each connection attempt. If the reconnect limit is reached after clean closes, the loop returns `EventLoopExit::ReconnectLimitReached`; if the final retryable failure is a transport error, the loop returns that error. WebSocket ping frames are answered by `WebSocketConnection`; packet reassembly for `sum > 1` and timer-driven application heartbeat remain later Channel event-layer work.

## Event Mapping

`parse_lark_event_payload` maps the official Lark/Feishu event and callback envelope into `ChannelEvent`:

- `im.message.receive_v1` -> `ChannelEvent::Message`
- `card.action.trigger` -> `ChannelEvent::CardAction`
- other event types -> `ChannelEvent::Unknown`

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
- `MessageContent::Card` -> `msg_type=interactive`
- `MessageContent::Custom` -> caller-provided `msg_type`
- `content` is serialized as the JSON string required by the official API
- `uuid` comes from `MessageCreateOptions`

`OpenApiClient::reply_message` maps SDK message types to the official reply-message API:

- `MessageId` -> path field `{message_id}`
- `MessageContent::Text` -> `msg_type=text`
- `MessageContent::Card` -> `msg_type=interactive`
- `MessageContent::Custom` -> caller-provided `msg_type`
- `content` is serialized as the JSON string required by the official API
- `uuid` and `reply_in_thread` come from `MessageReplyOptions`

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
- packet reassembly for long-connection events split across multiple frames
- automatic event dispatch and timer-driven heartbeat
- a complete Lark/Feishu OpenAPI surface
