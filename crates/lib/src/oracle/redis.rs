use super::{PriceOracle, PriceSource, TokenPrice};
use crate::{constant::SOL_MINT, error::KoraError};
use redis::AsyncCommands;
use reqwest::Client;
use rust_decimal::Decimal;
use std::collections::HashMap;

const REDIS_DEFAULT_CONFIDENCE: f64 = 0.8;
const DEFAULT_KEY_PREFIX: &str = "price";

pub struct RedisPriceOracle {
    client: redis::Client,
    key_prefix: String,
}

impl RedisPriceOracle {
    pub fn new() -> Result<Self, KoraError> {
        let url = std::env::var("REDIS_URL").map_err(|_| {
            log::error!("Redis URL not found. Set REDIS_URL environment variable.");
            KoraError::ConfigError(
                "Redis URL not found. Set REDIS_URL environment variable".to_string(),
            )
        })?;

        let key_prefix =
            std::env::var("REDIS_PRICE_PREFIX").unwrap_or_else(|_| DEFAULT_KEY_PREFIX.to_string());

        let client = redis::Client::open(url).map_err(|e| {
            KoraError::ConfigError(format!("Failed to create Redis client: {e}"))
        })?;

        Ok(Self { client, key_prefix })
    }

    async fn fetch_price_usd(&self, mint: &str) -> Result<f64, KoraError> {
        let mut conn = self.client.get_multiplexed_async_connection().await.map_err(|e| {
            KoraError::RpcError(format!("Failed to connect to Redis: {e}"))
        })?;

        let key = format!("{}:{}:latest", self.key_prefix, mint);
        let price_str: Option<String> = conn.hget(&key, "price_usd").await.map_err(|e| {
            KoraError::RpcError(format!("Failed to HGET Redis key {key}: {e}"))
        })?;

        let price_str = price_str.ok_or_else(|| {
            KoraError::RpcError(format!("No price data in Redis for mint {mint}"))
        })?;

        let price: f64 = price_str.parse().map_err(|e| {
            KoraError::RpcError(format!("Failed to parse Redis price for mint {mint}: {e}"))
        })?;

        if !price.is_finite() || price <= 0.0 {
            return Err(KoraError::RpcError(format!(
                "Invalid price value in Redis for mint {mint}: {price}"
            )));
        }

        Ok(price)
    }
}

#[async_trait::async_trait]
impl PriceOracle for RedisPriceOracle {
    async fn get_price(
        &self,
        _client: &Client,
        mint_address: &str,
    ) -> Result<TokenPrice, KoraError> {
        let prices = self.get_prices(_client, &[mint_address.to_string()]).await?;

        prices.get(mint_address).cloned().ok_or_else(|| {
            KoraError::RpcError(format!("No price data from Redis for mint {mint_address}"))
        })
    }

    async fn get_prices(
        &self,
        _client: &Client,
        mint_addresses: &[String],
    ) -> Result<HashMap<String, TokenPrice>, KoraError> {
        if mint_addresses.is_empty() {
            return Ok(HashMap::new());
        }

        let sol_usd = self.fetch_price_usd(SOL_MINT).await.map_err(|_| {
            KoraError::RpcError("No SOL price data in Redis — cannot convert to SOL-denominated prices".to_string())
        })?;

        let sol_usd_decimal = Decimal::from_f64_retain(sol_usd).ok_or_else(|| {
            KoraError::RpcError("Invalid SOL price from Redis".to_string())
        })?;

        let mut result = HashMap::new();
        for mint_address in mint_addresses {
            let token_usd = self.fetch_price_usd(mint_address).await?;

            let token_usd_decimal = Decimal::from_f64_retain(token_usd).ok_or_else(|| {
                KoraError::RpcError(format!("Invalid token price for mint {mint_address}"))
            })?;

            let price_in_sol = token_usd_decimal / sol_usd_decimal;

            result.insert(
                mint_address.clone(),
                TokenPrice {
                    price: price_in_sol,
                    confidence: REDIS_DEFAULT_CONFIDENCE,
                    source: PriceSource::Redis,
                    block_id: None,
                },
            );
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_fails_without_redis_url() {
        std::env::remove_var("REDIS_URL");
        let result = RedisPriceOracle::new();
        assert!(result.is_err());
        assert!(matches!(result.err(), Some(KoraError::ConfigError(_))));
    }

    #[test]
    fn test_new_succeeds_with_redis_url() {
        std::env::set_var("REDIS_URL", "redis://localhost:6379");
        let result = RedisPriceOracle::new();
        assert!(result.is_ok());
        std::env::remove_var("REDIS_URL");
    }

    #[test]
    fn test_default_key_prefix() {
        std::env::set_var("REDIS_URL", "redis://localhost:6379");
        std::env::remove_var("REDIS_PRICE_PREFIX");
        let oracle = RedisPriceOracle::new().unwrap();
        assert_eq!(oracle.key_prefix, "price");
        std::env::remove_var("REDIS_URL");
    }

    #[test]
    fn test_custom_key_prefix() {
        std::env::set_var("REDIS_URL", "redis://localhost:6379");
        std::env::set_var("REDIS_PRICE_PREFIX", "custom_prefix");
        let oracle = RedisPriceOracle::new().unwrap();
        assert_eq!(oracle.key_prefix, "custom_prefix");
        std::env::remove_var("REDIS_URL");
        std::env::remove_var("REDIS_PRICE_PREFIX");
    }
}
