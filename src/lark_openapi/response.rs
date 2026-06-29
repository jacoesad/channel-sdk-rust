use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Error, Result};

use super::HttpResponse;

pub(super) fn parse_openapi_response<R>(response: HttpResponse) -> Result<R>
where
    R: DeserializeOwned,
{
    if let Some(code) = response.body.get("code").and_then(Value::as_i64) {
        if code != 0 {
            let message = response
                .body
                .get("msg")
                .or_else(|| response.body.get("message"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            return Err(Error::Api { code, message });
        }
    }

    if !(200..300).contains(&response.status) {
        return Err(Error::Transport(format!(
            "http status {} from OpenAPI",
            response.status
        )));
    }

    serde_json::from_value(response.body).map_err(Error::from)
}
