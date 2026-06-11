//! Redis-backed sliding-window rate limit, keyed by owner address.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use crate::error::AppError;

#[derive(Clone)]
pub struct RateLimiter {
    conn: ConnectionManager,
    requests_per_minute: u32,
}

impl RateLimiter {
    pub fn new(conn: ConnectionManager, requests_per_minute: u32) -> Self {
        Self {
            conn,
            requests_per_minute,
        }
    }

    pub async fn check(&self, owner_address: &str) -> Result<(), AppError> {
        let mut conn = self.conn.clone();
        let key = format!("cr:rl:{owner_address}");
        let count: u32 = conn
            .incr(&key, 1)
            .await
            .map_err(|e| AppError::Internal(format!("redis incr: {e}")))?;
        if count == 1 {
            let _: () = conn
                .expire(&key, 60)
                .await
                .map_err(|e| AppError::Internal(format!("redis expire: {e}")))?;
        }
        if count > self.requests_per_minute {
            return Err(AppError::RateLimited);
        }
        Ok(())
    }
}
