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

Official docs:

- [App Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/app_access_token_internal.md)
- [Tenant Access Token](https://open.feishu.cn/document/server-docs/authentication-management/access-token/tenant_access_token_internal.md)
- [Create Message](https://open.feishu.cn/document/server-docs/im-v1/message/create.md)
- [Reply Message](https://open.feishu.cn/document/server-docs/im-v1/message/reply.md)
- [Use long connections to receive events](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case.md)

## WebSocket Endpoint Mapping

`OpenApiClient::websocket_endpoint` maps to the long-connection endpoint used by the official SDK family:

- request path: `POST /callback/ws/endpoint`
- request body: `AppID` and `AppSecret`
- response `data.URL` becomes `WebSocketEndpoint::url`
- response `data.ClientConfig` becomes `WebSocketClientConfig`

The endpoint URL is validated as `ws` or `wss` and must include the `device_id` and `service_id` query fields used by the long-connection protocol.

With the optional `websocket` feature enabled, `TokioTungsteniteWebSocketTransport` can connect to the endpoint and read/write raw `WebSocketFrame` values. Event acknowledgement, reconnect policy, and event normalization are intentionally left to the higher-level Channel event layer.

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
- event acknowledgement, reconnect policy, and normalized WebSocket events
- a complete Lark/Feishu OpenAPI surface
