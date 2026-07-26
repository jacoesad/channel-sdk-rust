use std::fmt;

use serde::{Deserialize, Serialize};
use url::Url;

use crate::debug::Redacted;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub domain: Domain,
    #[serde(default = "default_source")]
    pub source: String,
}

impl fmt::Debug for ChannelConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ChannelConfig")
            .field("app_id", &self.app_id)
            .field("app_secret", &Redacted)
            .field("domain", &self.domain)
            .field("source", &self.source)
            .finish()
    }
}

impl ChannelConfig {
    pub fn new(app_id: impl Into<String>, app_secret: impl Into<String>) -> Self {
        Self {
            app_id: app_id.into(),
            app_secret: app_secret.into(),
            domain: Domain::default(),
            source: default_source(),
        }
    }

    pub fn base_url(&self) -> Url {
        self.domain.base_url()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Domain {
    #[default]
    Feishu,
    Lark,
}

impl Domain {
    pub fn base_url(self) -> Url {
        match self {
            Domain::Feishu => Url::parse("https://open.feishu.cn").expect("valid feishu url"),
            Domain::Lark => Url::parse("https://open.larksuite.com").expect("valid lark url"),
        }
    }
}

fn default_source() -> String {
    env!("CARGO_PKG_NAME").to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_the_app_secret() {
        let debug = format!("{:?}", ChannelConfig::new("cli_test", "secret-value"));

        assert!(debug.contains("cli_test"));
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("secret-value"));
    }
}
