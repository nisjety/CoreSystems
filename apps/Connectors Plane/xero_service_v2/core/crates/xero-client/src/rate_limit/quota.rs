//! Optional Redis coordination. The app key is an explicit shared-app cohort;
//! every process using one Xero app must use the same key and Redis database.

use super::{Error, Result};
use redis::{aio::ConnectionManager, ConnectionAddr, Script};
use std::{fmt, net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::OnceCell;
use tracing::warn;
use uuid::Uuid;

const LEASE_MS: i64 = 120_000; // HTTP timeout is 60s; stale leases expire after 2 minutes.
const MAX_WAIT_MS: u64 = 24 * 60 * 60 * 1000;
const REDIS_TIMEOUT: Duration = Duration::from_secs(5);

pub(super) struct RedisQuota {
    client: redis::Client,
    connection: OnceCell<ConnectionManager>,
    prefix: String,
}

impl fmt::Debug for RedisQuota {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisQuota")
            .field("configured", &true)
            .finish()
    }
}

impl RedisQuota {
    pub(super) fn from_env() -> Result<Option<Self>> {
        let url = std::env::var_os("XERO_REDIS_URL")
            .map(|value| value.into_string().map_err(|_| unavailable()))
            .transpose()?;
        let cohort = std::env::var_os("XERO_REDIS_APP_KEY")
            .map(|value| value.into_string().map_err(|_| unavailable()))
            .transpose()?;
        Self::from_settings(url.as_deref(), cohort.as_deref())
    }

    pub(super) fn validate_startup(&self) -> Result<()> {
        let connection = self
            .client
            .get_connection_with_timeout(REDIS_TIMEOUT)
            .map_err(|_| unavailable())?;
        connection
            .set_read_timeout(Some(REDIS_TIMEOUT))
            .map_err(|_| unavailable())?;
        connection
            .set_write_timeout(Some(REDIS_TIMEOUT))
            .map_err(|_| unavailable())?;
        let mut connection = connection;
        let pong = redis::cmd("PING")
            .query::<String>(&mut connection)
            .map_err(|_| unavailable())?;
        if pong == "PONG" {
            // PING is insufficient: quota acquisition executes Lua and writes
            // Redis keys. Verify those exact capabilities before accepting a
            // worker into a coordinated cohort.
            let key = format!("{}:startup:{}", self.prefix, Uuid::new_v4());
            let _missing_script = redis::cmd("EVALSHA")
                .arg("0000000000000000000000000000000000000000")
                .arg(1)
                .arg(&key)
                .arg("1")
                .query::<String>(&mut connection)
                .err()
                .filter(|error| error.kind() == redis::ErrorKind::NoScriptError)
                .ok_or_else(unavailable)?;
            let result = redis::cmd("EVAL")
                .arg("return redis.call('SET', KEYS[1], ARGV[1], 'PX', 1000)")
                .arg(1)
                .arg(&key)
                .arg("1")
                .query::<String>(&mut connection)
                .map_err(|_| unavailable())?;
            if result != "OK" {
                return Err(unavailable());
            }
            redis::cmd("DEL")
                .arg(&key)
                .query::<()>(&mut connection)
                .map_err(|_| unavailable())?;
            Ok(())
        } else {
            Err(unavailable())
        }
    }

    pub(super) fn from_settings(url: Option<&str>, cohort: Option<&str>) -> Result<Option<Self>> {
        match (url, cohort) {
            (None, None) => Ok(None),
            (Some(url), Some(cohort)) if !url.is_empty() && !cohort.is_empty() => {
                if !cohort
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
                    || cohort.len() > 80
                {
                    return Err(Error::XeroApi("invalid Redis app cohort key".into()));
                }
                let client = redis::Client::open(url).map_err(|_| unavailable())?;
                match &client.get_connection_info().addr {
                    ConnectionAddr::Tcp(host, _) => {
                        let loopback = host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
                        if !loopback {
                            return Err(Error::Config(
                                "Redis quota requires TLS for non-loopback TCP".into(),
                            ));
                        }
                    }
                    ConnectionAddr::TcpTls { insecure: true, .. } => {
                        return Err(Error::Config(
                            "Redis quota requires TLS certificate verification".into(),
                        ));
                    }
                    ConnectionAddr::TcpTls {
                        insecure: false, ..
                    }
                    | ConnectionAddr::Unix(_) => {}
                }
                Ok(Some(Self {
                    client,
                    connection: OnceCell::new(),
                    prefix: format!("xero:quota:{{{cohort}}}"),
                }))
            }
            _ => Err(Error::XeroApi(
                "Redis URL and app cohort key must both be configured".into(),
            )),
        }
    }

    async fn connection(&self) -> Result<ConnectionManager> {
        self.connection
            .get_or_try_init(|| async {
                tokio::time::timeout(REDIS_TIMEOUT, ConnectionManager::new(self.client.clone()))
                    .await
                    .map_err(|_| unavailable())?
                    .map_err(|_| unavailable())
            })
            .await
            .cloned()
    }

    fn keys(&self, tenant: &str) -> Result<[String; 7]> {
        if tenant.is_empty() || tenant.len() > 128 {
            return Err(Error::XeroApi(
                "invalid tenant ID for quota coordination".into(),
            ));
        }
        // Hex encoding is injective and prevents key injection for external IDs.
        let tenant = tenant
            .as_bytes()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let base = format!("{}:tenant:{tenant}", self.prefix);
        Ok([
            format!("{base}:leases"),
            format!("{base}:minute"),
            format!("{base}:day"),
            format!("{}:app:minute", self.prefix),
            format!("{base}:gate"),
            format!("{}:app:gate", self.prefix),
            format!("{base}:cooldown"),
        ])
    }

    pub(super) async fn acquire(self: &Arc<Self>, tenant: &str) -> Result<RemoteLease> {
        let keys = self.keys(tenant)?;
        let id = Uuid::new_v4().to_string();
        loop {
            let mut connection = self.connection().await?;
            let query = Script::new(ACQUIRE);
            let (status, wait_ms): (i64, i64) = tokio::time::timeout(
                REDIS_TIMEOUT,
                query
                    .key(&keys[0])
                    .key(&keys[1])
                    .key(&keys[2])
                    .key(&keys[3])
                    .key(&keys[4])
                    .key(&keys[5])
                    .key(&keys[6])
                    .arg(&id)
                    .arg(LEASE_MS)
                    .invoke_async(&mut connection),
            )
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
            match status {
                1 => {
                    return Ok(RemoteLease {
                        quota: self.clone(),
                        lease_key: keys[0].clone(),
                        id,
                    })
                }
                -1 => {
                    return Err(Error::XeroApi(
                        "daily Xero API allowance exhausted; later probe required".into(),
                    ))
                }
                0 => {
                    tokio::time::sleep(Duration::from_millis(wait_ms.clamp(50, 60_000) as u64))
                        .await
                }
                _ => return Err(unavailable()),
            }
        }
    }

    pub(super) async fn observe(
        &self,
        tenant: &str,
        minute: Option<u32>,
        day: Option<u32>,
        app: Option<u32>,
    ) -> Result<()> {
        if minute.is_none() && day.is_none() && app.is_none() {
            return Ok(());
        }
        let keys = self.keys(tenant)?;
        let mut connection = self.connection().await?;
        let query = Script::new(OBSERVE);
        tokio::time::timeout(
            REDIS_TIMEOUT,
            query
                .key(&keys[2])
                .key(&keys[4])
                .key(&keys[5])
                .arg(minute.map(i64::from).unwrap_or(-1))
                .arg(day.map(i64::from).unwrap_or(-1))
                .arg(app.map(i64::from).unwrap_or(-1))
                .invoke_async::<()>(&mut connection),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())
    }

    pub(super) async fn defer(&self, tenant: &str, wait: Duration) -> Result<()> {
        let millis = u64::try_from(wait.as_millis()).unwrap_or(u64::MAX);
        if millis > MAX_WAIT_MS {
            return Err(Error::XeroApi(
                "Retry-After exceeds supported 24-hour cooldown".into(),
            ));
        }
        let keys = self.keys(tenant)?;
        let mut connection = self.connection().await?;
        let query = Script::new(DEFER);
        tokio::time::timeout(
            REDIS_TIMEOUT,
            query
                .key(&keys[2])
                .key(&keys[6])
                .arg(millis)
                .invoke_async::<()>(&mut connection),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())
    }

    async fn release(&self, lease_key: &str, id: &str) -> Result<()> {
        let mut connection = self.connection().await?;
        let query = Script::new(RELEASE);
        tokio::time::timeout(
            REDIS_TIMEOUT,
            query
                .key(lease_key)
                .arg(id)
                .invoke_async::<()>(&mut connection),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())
    }
}

pub(super) struct RemoteLease {
    quota: Arc<RedisQuota>,
    lease_key: String,
    id: String,
}

impl Drop for RemoteLease {
    fn drop(&mut self) {
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            let quota = self.quota.clone();
            let key = self.lease_key.clone();
            let id = self.id.clone();
            runtime.spawn(async move {
                if quota.release(&key, &id).await.is_err() {
                    warn!("distributed quota lease release failed; bounded lease will expire");
                }
            });
        } else {
            warn!("distributed quota lease release could not run; bounded lease will expire");
        }
    }
}

fn unavailable() -> Error {
    Error::XeroApi("distributed quota unavailable".into())
}

const ACQUIRE: &str = r#"
local function quota_number(key, field)
  local raw = redis.call('HGET', key, field)
  if not raw then return nil end
  local value = tonumber(raw)
  if not value or value < 0 or value % 1 ~= 0 then error('corrupt quota state') end
  return value
end
for _, key in ipairs({KEYS[3], KEYS[5], KEYS[6]}) do
  for _, field in ipairs({'remaining', 'started', 'probe_at', 'probe_sent'}) do quota_number(key, field) end
end
local time = redis.call('TIME')
local now = time[1] * 1000 + math.floor(time[2] / 1000)
redis.call('ZREMRANGEBYSCORE', KEYS[1], '-inf', now)
redis.call('ZREMRANGEBYSCORE', KEYS[2], '-inf', now - 60000)
redis.call('ZREMRANGEBYSCORE', KEYS[4], '-inf', now - 60000)
local until_raw = redis.call('GET', KEYS[7])
local until_time = until_raw and tonumber(until_raw) or 0
if not until_time or until_time < 0 then error('corrupt quota state') end
if until_time > now then return {0, until_time - now} end
if redis.call('ZCARD', KEYS[1]) >= 5 then return {0, 1000} end
if redis.call('ZCARD', KEYS[2]) >= 60 then return {0, 1000} end
if redis.call('ZCARD', KEYS[4]) >= 10000 then return {0, 1000} end
for _, key in ipairs({KEYS[5], KEYS[6]}) do
  local started = quota_number(key, 'started') or 0
  if started > 0 and now - started < 60000 then
    if (quota_number(key, 'remaining') or -1) == 0 then return {0, started + 60000 - now} end
  else redis.call('DEL', key) end
end
local day = quota_number(KEYS[3], 'remaining') or -1
if day == 0 then
  local probe_at = quota_number(KEYS[3], 'probe_at') or 0
  if probe_at == 0 or now < probe_at then return {-1, 0} end
  redis.call('HSET', KEYS[3], 'probe_at', now + 60000, 'probe_sent', 1)
elseif day > 0 then
  redis.call('HINCRBY', KEYS[3], 'remaining', -1)
  if day == 1 then
    local started = quota_number(KEYS[3], 'started') or now
    redis.call('HSET', KEYS[3], 'probe_at', started + 86400000)
  end
end
for _, key in ipairs({KEYS[5], KEYS[6]}) do
  local remaining = quota_number(key, 'remaining') or -1
  if remaining > 0 then redis.call('HINCRBY', key, 'remaining', -1) end
end
redis.call('ZADD', KEYS[1], now + tonumber(ARGV[2]), ARGV[1])
redis.call('PEXPIRE', KEYS[1], tonumber(ARGV[2]) + 1000)
redis.call('ZADD', KEYS[2], now, ARGV[1])
redis.call('PEXPIRE', KEYS[2], 61000)
redis.call('ZADD', KEYS[4], now, ARGV[1])
redis.call('PEXPIRE', KEYS[4], 61000)
redis.call('PEXPIRE', KEYS[3], 90000000)
return {1, 0}
"#;

const OBSERVE: &str = r#"
local function quota_number(key, field)
  local raw = redis.call('HGET', key, field)
  if not raw then return nil end
  local value = tonumber(raw)
  if not value or value < 0 or value % 1 ~= 0 then error('corrupt quota state') end
  return value
end
for _, key in ipairs(KEYS) do
  for _, field in ipairs({'remaining', 'started', 'probe_at', 'probe_sent'}) do quota_number(key, field) end
end
local time = redis.call('TIME')
local now = time[1] * 1000 + math.floor(time[2] / 1000)
for _, pair in ipairs({{KEYS[2], tonumber(ARGV[1])}, {KEYS[3], tonumber(ARGV[3])}}) do
  local key, observed = pair[1], pair[2]
  if observed >= 0 then
    local started = quota_number(key, 'started') or 0
    if observed == 0 or started == 0 or now - started >= 60000 then
      redis.call('HSET', key, 'started', now, 'remaining', observed)
    else
      local old = quota_number(key, 'remaining') or observed
      redis.call('HSET', key, 'remaining', math.min(old, observed))
    end
    redis.call('PEXPIRE', key, 61000)
  end
end
local observed_day = tonumber(ARGV[2])
if observed_day >= 0 then
  local started = quota_number(KEYS[1], 'started') or 0
  local probe_sent = quota_number(KEYS[1], 'probe_sent') or 0
  if probe_sent > 1 then error('corrupt quota state') end
  if started == 0 or now - started >= 86400000 or probe_sent == 1 then
    redis.call('HSET', KEYS[1], 'started', now, 'remaining', observed_day)
  else
    local old = quota_number(KEYS[1], 'remaining') or observed_day
    redis.call('HSET', KEYS[1], 'remaining', math.min(old, observed_day))
  end
  redis.call('HDEL', KEYS[1], 'probe_sent')
  if observed_day == 0 then
    local origin = quota_number(KEYS[1], 'started') or now
    redis.call('HSETNX', KEYS[1], 'probe_at', origin + 86400000)
  else redis.call('HDEL', KEYS[1], 'probe_at') end
  redis.call('PEXPIRE', KEYS[1], 90000000)
end
return redis.status_reply('OK')
"#;

const DEFER: &str = r#"
local function quota_number(key, field)
  local raw = redis.call('HGET', key, field)
  if not raw then return nil end
  local value = tonumber(raw)
  if not value or value < 0 or value % 1 ~= 0 then error('corrupt quota state') end
  return value
end
for _, field in ipairs({'remaining', 'started', 'probe_at', 'probe_sent'}) do quota_number(KEYS[1], field) end
local time = redis.call('TIME')
local now = time[1] * 1000 + math.floor(time[2] / 1000)
local until_raw = redis.call('GET', KEYS[2])
local until_time = until_raw and tonumber(until_raw) or 0
if not until_time or until_time < 0 then error('corrupt quota state') end
local deadline = math.max(until_time, now + tonumber(ARGV[1]))
redis.call('SET', KEYS[2], deadline, 'PX', math.max(1000, deadline - now + 1000))
if (quota_number(KEYS[1], 'remaining') or -1) == 0 then
  redis.call('HSET', KEYS[1], 'probe_at', deadline)
  redis.call('PEXPIRE', KEYS[1], 90000000)
end
return redis.status_reply('OK')
"#;

const RELEASE: &str = r#"return redis.call('ZREM', KEYS[1], ARGV[1])"#;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_tcp_requires_verified_tls() {
        assert!(RedisQuota::from_settings(Some("redis://10.0.0.5:6379/"), Some("cohort")).is_err());
        assert!(
            RedisQuota::from_settings(Some("redis://example.com:6379/"), Some("cohort")).is_err()
        );
        assert!(
            RedisQuota::from_settings(Some("redis://localhost:6379/"), Some("cohort")).is_err()
        );
        assert!(RedisQuota::from_settings(
            Some("rediss://example.com:6379/#insecure"),
            Some("cohort")
        )
        .is_err());
        assert!(
            RedisQuota::from_settings(Some("rediss://example.com:6379/"), Some("cohort")).is_ok()
        );
        assert!(RedisQuota::from_settings(Some("redis://127.0.0.1:6379/"), Some("cohort")).is_ok());
    }

    #[test]
    fn tenant_key_encoding_is_unique_and_bounded() {
        let quota = RedisQuota::from_settings(Some("redis://127.0.0.1/"), Some("cohort"))
            .unwrap()
            .unwrap();
        assert_ne!(quota.keys("a:b").unwrap()[0], quota.keys("a-b").unwrap()[0]);
        assert!(quota.keys("").is_err());
        assert!(quota.keys(&"x".repeat(129)).is_err());
    }

    fn fixture() -> Option<(Arc<RedisQuota>, Arc<RedisQuota>)> {
        let url = std::env::var("XERO_TEST_REDIS_URL").ok()?;
        let cohort = format!("test-{}", Uuid::new_v4());
        Some((
            Arc::new(
                RedisQuota::from_settings(Some(&url), Some(&cohort))
                    .unwrap()
                    .unwrap(),
            ),
            Arc::new(
                RedisQuota::from_settings(Some(&url), Some(&cohort))
                    .unwrap()
                    .unwrap(),
            ),
        ))
    }

    #[tokio::test]
    async fn concurrent_leases_are_shared_and_released() {
        let Some((a, b)) = fixture() else { return };
        let tenant = Uuid::new_v4().to_string();
        let mut leases = Vec::new();
        for _ in 0..5 {
            leases.push(a.acquire(&tenant).await.unwrap());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(150), b.acquire(&tenant))
                .await
                .is_err()
        );
        drop(leases.pop());
        let lease = tokio::time::timeout(Duration::from_secs(3), b.acquire(&tenant))
            .await
            .unwrap()
            .unwrap();
        drop(lease);
    }

    #[tokio::test]
    async fn tenant_rolling_minute_is_shared_across_instances() {
        let Some((a, b)) = fixture() else { return };
        let tenant = Uuid::new_v4().to_string();
        for _ in 0..60 {
            drop(a.acquire(&tenant).await.unwrap());
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(150), b.acquire(&tenant))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn late_zero_app_header_restarts_distributed_gate() {
        let Some((a, b)) = fixture() else { return };
        let tenant = Uuid::new_v4().to_string();
        let keys = a.keys(&tenant).unwrap();
        let mut connection = a.connection().await.unwrap();
        let _: () = redis::cmd("HSET")
            .arg(&keys[5])
            .arg("started")
            .arg(1)
            .arg("remaining")
            .arg(1)
            .query_async(&mut connection)
            .await
            .unwrap();
        a.observe(&tenant, None, None, Some(0)).await.unwrap();
        let started: i64 = redis::cmd("HGET")
            .arg(&keys[5])
            .arg("started")
            .query_async(&mut connection)
            .await
            .unwrap();
        assert!(started > 1);
        assert!(
            tokio::time::timeout(Duration::from_millis(150), b.acquire(&tenant))
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn corrupted_persisted_quota_rejects_before_reservation() {
        let Some((a, b)) = fixture() else { return };
        let tenant = Uuid::new_v4().to_string();
        let keys = a.keys(&tenant).unwrap();
        let mut connection = a.connection().await.unwrap();
        let _: () = redis::cmd("HSET")
            .arg(&keys[2])
            .arg("remaining")
            .arg(-1)
            .query_async(&mut connection)
            .await
            .unwrap();
        assert!(a.acquire(&tenant).await.is_err());
        let leases: usize = redis::cmd("ZCARD")
            .arg(&keys[0])
            .query_async(&mut connection)
            .await
            .unwrap();
        assert_eq!(leases, 0);
        let _: () = redis::cmd("DEL")
            .arg(&keys[2])
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: () = redis::cmd("HSET")
            .arg(&keys[5])
            .arg("remaining")
            .arg("corrupt")
            .query_async(&mut connection)
            .await
            .unwrap();
        assert!(b.acquire(&tenant).await.is_err());
        assert!(a.observe(&tenant, None, None, Some(1)).await.is_err());
        let _: () = redis::cmd("DEL")
            .arg(&keys[5])
            .query_async(&mut connection)
            .await
            .unwrap();
        let _: () = redis::cmd("HSET")
            .arg(&keys[2])
            .arg("probe_at")
            .arg("invalid")
            .query_async(&mut connection)
            .await
            .unwrap();
        assert!(a.defer(&tenant, Duration::from_secs(1)).await.is_err());
    }

    #[tokio::test]
    async fn shared_cooldown_app_gate_and_daily_single_probe() {
        let Some((a, b)) = fixture() else { return };
        let tenant = Uuid::new_v4().to_string();
        a.defer(&tenant, Duration::from_millis(250)).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), b.acquire(&tenant))
                .await
                .is_err()
        );
        drop(b.acquire(&tenant).await.unwrap());

        a.observe(&tenant, None, None, Some(0)).await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(100), b.acquire(&tenant))
                .await
                .is_err()
        );
        // Clear only the test fixture's app gate before exercising daily quota.
        let keys = a.keys(&tenant).unwrap();
        let mut connection = a.connection().await.unwrap();
        let _: () = redis::cmd("DEL")
            .arg(&keys[5])
            .query_async(&mut connection)
            .await
            .unwrap();
        a.observe(&tenant, None, Some(0), None).await.unwrap();
        assert!(b.acquire(&tenant).await.is_err());
        b.defer(&tenant, Duration::from_millis(100)).await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;
        drop(a.acquire(&tenant).await.unwrap());
        assert!(b.acquire(&tenant).await.is_err());
    }

    #[tokio::test]
    async fn configured_unavailable_redis_fails_closed_without_url_in_error() {
        let quota = Arc::new(
            RedisQuota::from_settings(
                Some("redis://secret@127.0.0.1:1/"),
                Some("unavailable-test"),
            )
            .unwrap()
            .unwrap(),
        );
        let message = quota.acquire("tenant").await.err().unwrap().to_string();
        assert!(message.contains("unavailable"));
        assert!(!message.contains("secret"));
    }
}
