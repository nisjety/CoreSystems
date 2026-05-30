use std::time::Duration;

use sonic_channel::{
    Dest, IngestChannel, PushRequest, QueryRequest, SearchChannel, SonicChannel, SuggestRequest,
};

use crate::{config::SonicSettings, AppError, AppResult};

#[derive(Debug, Clone)]
pub struct SonicClient {
    enabled: bool,
    addr: String,
    password: String,
    timeout: Duration,
}

impl SonicClient {
    pub fn new(settings: SonicSettings) -> Self {
        Self {
            enabled: settings.enabled,
            addr: settings.addr,
            password: settings.password,
            timeout: settings.timeout,
        }
    }

    pub fn enabled(&self) -> bool {
        self.enabled
    }

    pub async fn push(
        &self,
        collection: String,
        bucket: String,
        object: String,
        text: String,
    ) -> AppResult<()> {
        if !self.enabled {
            return Ok(());
        }

        let addr = self.addr.clone();
        let password = self.password.clone();
        self.run_blocking(move || {
            let channel = IngestChannel::start(addr.as_str(), password.as_str())
                .map_err(|error| error.to_string())?;
            let dest = Dest::col_buc(collection, bucket).obj(object);
            channel
                .push(PushRequest::new(dest, text))
                .map_err(|error| error.to_string())?;
            let _ = channel.quit();
            Ok(())
        })
        .await
    }

    pub async fn query(
        &self,
        collection: String,
        bucket: String,
        terms: String,
        limit: usize,
    ) -> AppResult<Vec<String>> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        let addr = self.addr.clone();
        let password = self.password.clone();
        self.run_blocking(move || {
            let channel = SearchChannel::start(addr.as_str(), password.as_str())
                .map_err(|error| error.to_string())?;
            let request = QueryRequest::new(Dest::col_buc(collection, bucket), terms).limit(limit);
            let result = channel.query(request).map_err(|error| error.to_string())?;
            let _ = channel.quit();
            Ok(result)
        })
        .await
    }

    pub async fn suggest_words(
        &self,
        collection: String,
        bucket: String,
        word: String,
        limit: usize,
    ) -> AppResult<Vec<String>> {
        if !self.enabled {
            return Ok(Vec::new());
        }

        let addr = self.addr.clone();
        let password = self.password.clone();
        self.run_blocking(move || {
            let channel = SearchChannel::start(addr.as_str(), password.as_str())
                .map_err(|error| error.to_string())?;
            let request = SuggestRequest::new(Dest::col_buc(collection, bucket), word).limit(limit);
            let result = channel
                .suggest(request)
                .map_err(|error| error.to_string())?;
            let _ = channel.quit();
            Ok(result)
        })
        .await
    }

    async fn run_blocking<T, F>(&self, f: F) -> AppResult<T>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, String> + Send + 'static,
    {
        let task = tokio::task::spawn_blocking(f);
        match tokio::time::timeout(self.timeout, task).await {
            Ok(join_result) => join_result?.map_err(AppError::Sonic),
            Err(_) => Err(AppError::Sonic(format!(
                "sonic operation timed out after {}s",
                self.timeout.as_secs()
            ))),
        }
    }
}
