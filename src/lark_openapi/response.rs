use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Error, Result};

use super::HttpResponse;

pub(super) fn parse_openapi_response<R>(response: HttpResponse) -> Result<R>
where
    R: DeserializeOwned,
{
    if !(200..300).contains(&response.status) {
        return Err(Error::HttpStatus {
            status: response.status,
        });
    }

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

    serde_json::from_value(response.body).map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn response_decode_errors_do_not_render_raw_values() {
        let error =
            parse_openapi_response::<bool>(HttpResponse::json(200, json!("response-secret")))
                .expect_err("invalid response type should fail");
        let rendered = format!("{error:?} {error}");

        assert!(matches!(error, Error::Serde(_)));
        assert!(!rendered.contains("response-secret"));
    }
}
