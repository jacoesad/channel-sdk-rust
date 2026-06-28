use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub app_id: String,
    pub app_secret: String,
    #[serde(default)]
    pub domain: Domain,
    #[serde(default = "default_source")]
    pub source: String,
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
