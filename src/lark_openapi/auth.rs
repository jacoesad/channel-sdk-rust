use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::Result;

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

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct AppAccessTokenResponse {
    pub app_access_token: String,
    pub expire: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct TenantAccessTokenResponse {
    pub tenant_access_token: String,
    pub expire: u64,
}

#[derive(Debug, Serialize)]
struct SelfBuiltTokenRequest<'a> {
    app_id: &'a str,
    app_secret: &'a str,
}

#[derive(Debug)]
pub(super) struct AccessTokenCache {
    token: Mutex<Option<CachedAccessToken>>,
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

#[derive(Debug)]
struct CachedAccessToken {
    value: String,
    expires_at: Instant,
}
