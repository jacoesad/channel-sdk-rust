use std::fmt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::Result;
use crate::debug::Redacted;

use super::{OpenApiClient, OpenApiTransport};

const APP_ACCESS_TOKEN_PATH: &str = "/open-apis/auth/v3/app_access_token/internal";
const TENANT_ACCESS_TOKEN_PATH: &str = "/open-apis/auth/v3/tenant_access_token/internal";
const TOKEN_REFRESH_SKEW: Duration = Duration::from_secs(600);

impl<T> OpenApiClient<T>
where
    T: OpenApiTransport,
{
    pub async fn app_access_token(&self) -> Result<String> {
        let now = Instant::now();
        if let Some(token) = self.app_access_token_cache.get(now) {
            return Ok(token);
        }

        let token = self.request_app_access_token().await?;
        let access_token = token.app_access_token.clone();
        self.app_access_token_cache
            .store(access_token.clone(), token.expire, Instant::now());
        Ok(access_token)
    }

    pub async fn tenant_access_token(&self) -> Result<String> {
        let now = Instant::now();
        if let Some(token) = self.tenant_access_token_cache.get(now) {
            return Ok(token);
        }

        let token = self.request_tenant_access_token().await?;
        let access_token = token.tenant_access_token.clone();
        self.tenant_access_token_cache
            .store(access_token.clone(), token.expire, Instant::now());
        Ok(access_token)
    }

    async fn request_app_access_token(&self) -> Result<AppAccessTokenResponse> {
        self.post_openapi_json(
            APP_ACCESS_TOKEN_PATH,
            &SelfBuiltTokenRequest {
                app_id: &self.config.app_id,
                app_secret: &self.config.app_secret,
            },
        )
        .await
    }

    async fn request_tenant_access_token(&self) -> Result<TenantAccessTokenResponse> {
        self.post_openapi_json(
            TENANT_ACCESS_TOKEN_PATH,
            &SelfBuiltTokenRequest {
                app_id: &self.config.app_id,
                app_secret: &self.config.app_secret,
            },
        )
        .await
    }
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AppAccessTokenResponse {
    pub app_access_token: String,
    pub expire: u64,
}

impl fmt::Debug for AppAccessTokenResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AppAccessTokenResponse")
            .field("app_access_token", &Redacted)
            .field("expire", &self.expire)
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TenantAccessTokenResponse {
    pub tenant_access_token: String,
    pub expire: u64,
}

impl fmt::Debug for TenantAccessTokenResponse {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("TenantAccessTokenResponse")
            .field("tenant_access_token", &Redacted)
            .field("expire", &self.expire)
            .finish()
    }
}

#[derive(Serialize)]
struct SelfBuiltTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

pub(super) struct AccessTokenCache {
    token: Mutex<Option<CachedAccessToken>>,
}

impl fmt::Debug for AccessTokenCache {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AccessTokenCache")
            .finish_non_exhaustive()
    }
}

impl Default for AccessTokenCache {
    fn default() -> Self {
        Self {
            token: Mutex::new(None),
        }
    }
}

impl AccessTokenCache {
    fn get(&self, now: Instant) -> Option<String> {
        let token = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let token = token.as_ref()?;

        if token.expires_at > now + TOKEN_REFRESH_SKEW {
            Some(token.value.clone())
        } else {
            None
        }
    }

    fn store(&self, value: String, expire: u64, now: Instant) {
        let mut token = self
            .token
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *token = Some(CachedAccessToken {
            value,
            expires_at: now + Duration::from_secs(expire),
        });
    }
}

struct CachedAccessToken {
    value: String,
    expires_at: Instant,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_redacts_access_tokens_and_cached_values() {
        let app = AppAccessTokenResponse {
            app_access_token: "app-token-value".to_owned(),
            expire: 7200,
        };
        let tenant = TenantAccessTokenResponse {
            tenant_access_token: "tenant-token-value".to_owned(),
            expire: 7200,
        };
        let cache = AccessTokenCache::default();
        cache.store("cached-token-value".to_owned(), 7200, Instant::now());

        let debug = format!("{app:?} {tenant:?} {cache:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("app-token-value"));
        assert!(!debug.contains("tenant-token-value"));
        assert!(!debug.contains("cached-token-value"));
    }
}
