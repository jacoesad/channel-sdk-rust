use std::collections::BTreeMap;
use std::fmt;

pub(crate) struct Redacted;

impl fmt::Debug for Redacted {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("<redacted>")
    }
}

pub(crate) struct RedactedHeaders<'a>(pub(crate) &'a BTreeMap<String, String>);

impl fmt::Debug for RedactedHeaders<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut map = formatter.debug_map();
        for (name, value) in self.0 {
            if is_sensitive_header(name) {
                map.entry(name, &Redacted);
            } else {
                map.entry(name, value);
            }
        }
        map.finish()
    }
}

fn is_sensitive_header(name: &str) -> bool {
    [
        "authorization",
        "proxy-authorization",
        "cookie",
        "set-cookie",
        "x-api-key",
    ]
    .iter()
    .any(|sensitive| name.eq_ignore_ascii_case(sensitive))
}
