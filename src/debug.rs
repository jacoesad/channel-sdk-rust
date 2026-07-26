use std::collections::BTreeMap;
use std::fmt;

use serde_json::Value;
use url::Url;

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
        for name in self.0.keys() {
            map.entry(name, &Redacted);
        }
        map.finish()
    }
}

pub(crate) struct RedactedOption<'a, T>(pub(crate) &'a Option<T>);

impl<T> fmt::Debug for RedactedOption<'_, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(_) => formatter.debug_tuple("Some").field(&Redacted).finish(),
            None => formatter.write_str("None"),
        }
    }
}

pub(crate) struct RedactedUrl<'a>(pub(crate) &'a Url);

impl fmt::Debug for RedactedUrl<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Url")
            .field("scheme", &self.0.scheme())
            .field("host", &self.0.host_str())
            .field("port", &self.0.port())
            .finish()
    }
}

pub(crate) struct JsonSummary<'a>(pub(crate) &'a Value);

impl fmt::Debug for JsonSummary<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Value::Null => formatter.write_str("Null"),
            Value::Bool(_) => formatter.write_str("Bool(<redacted>)"),
            Value::Number(_) => formatter.write_str("Number(<redacted>)"),
            Value::String(value) => formatter
                .debug_struct("String")
                .field("chars", &value.chars().count())
                .finish(),
            Value::Array(values) => formatter
                .debug_struct("Array")
                .field("len", &values.len())
                .finish(),
            Value::Object(values) => formatter
                .debug_struct("Object")
                .field("fields", &values.len())
                .finish(),
        }
    }
}

pub(crate) struct OptionalJsonSummary<'a>(pub(crate) &'a Option<Value>);

impl fmt::Debug for OptionalJsonSummary<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Some(value) => formatter
                .debug_tuple("Some")
                .field(&JsonSummary(value))
                .finish(),
            None => formatter.write_str("None"),
        }
    }
}
