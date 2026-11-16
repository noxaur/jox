//! Client for roarr (https://github.com/noxaur/roarr), which fetches titles
//! for a Jellyfin library through Sonarr, Radarr and Prowlarr.
//!
//! Each server can name a roarr instance. jox signs in to it with the same
//! Jellyfin account (roarr's `AUTH_MODE=jellyfin`) and keeps the session
//! cookie roarr hands back, signing in again when it expires.

use std::{
    sync::{
        Arc,
        Mutex,
    },
    time::Duration,
};

use anyhow::{
    Result,
    anyhow,
    bail,
};
use arc_swap::ArcSwapOption;
use once_cell::sync::Lazy;
use reqwest::{
    Method,
    StatusCode,
    header,
};
use serde::{
    Deserialize,
    Serialize,
    de::DeserializeOwned,
};
use serde_json::json;
use url::Url;

use super::{
    Account,
    ReqClient,
};

pub static ROARR: Lazy<RoarrClient> = Lazy::new(RoarrClient::default);

/// Getting a title waits for indexers to settle, which roarr caps at two minutes.
const AUTO_TIMEOUT: Duration = Duration::from_secs(150);

struct Config {
    base: Url,
    username: String,
    password: String,
}

pub struct RoarrClient {
    config: ArcSwapOption<Config>,
    cookie: Mutex<Option<String>>,
    client: reqwest::Client,
}

impl Default for RoarrClient {
    fn default() -> Self {
        Self {
            config: ArcSwapOption::empty(),
            cookie: Mutex::new(None),
            client: ReqClient::build(),
        }
    }
}

impl RoarrClient {
    /// Points the client at the roarr of the server that just became active.
    pub fn configure(&self, account: &Account) {
        *self.cookie.lock().unwrap() = None;
        let base = Url::parse(&format!("{}/", account.roarr_url.trim_end_matches('/'))).ok();
        self.config.store(base.map(|base| {
            Arc::new(Config {
                base,
                username: account.username.clone(),
                password: account.password.clone(),
            })
        }));
    }

    pub fn is_configured(&self) -> bool {
        self.config.load().is_some()
    }

    /// Absolute URL for a poster roarr reports, which may be relative to roarr.
    pub fn image_url(&self, poster: &str) -> Option<String> {
        if poster.is_empty() {
            return None;
        }
        if poster.starts_with("http://") || poster.starts_with("https://") {
            return Some(poster.to_owned());
        }
        let config = self.config.load_full()?;
        config
            .base
            .join(poster.trim_start_matches('/'))
            .ok()
            .map(String::from)
    }

    pub async fn activity(&self) -> Result<Vec<ActivityItem>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            items: Vec<ActivityItem>,
        }
        Ok(self
            .json::<Response>(Method::GET, "api/activity", None)
            .await?
            .items)
    }

    pub async fn requests(&self) -> Result<Requests> {
        self.json(Method::GET, "api/requests?filter=all", None)
            .await
    }

    /// `approve` or `decline`.
    pub async fn request_action(&self, id: i64, action: &str) -> Result<()> {
        self.send(
            Method::POST,
            &format!("api/requests/{id}/{action}"),
            None,
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn discover(&self, query: &str) -> Result<Vec<DiscoverItem>> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default)]
            results: Vec<DiscoverItem>,
        }
        let mut path = Url::parse("roarr:/api/discover")?;
        path.query_pairs_mut().append_pair("q", query);
        let path = format!("api/discover?{}", path.query().unwrap_or_default());
        Ok(self.json::<Response>(Method::GET, &path, None).await?.results)
    }

    pub async fn series_by_tvdb(&self, tvdb: i64, tmdb: Option<i64>) -> Result<SeriesDetail> {
        let mut path = format!("api/tv/tvdb/{tvdb}");
        if let Some(tmdb) = tmdb {
            path.push_str(&format!("?tmdb={tmdb}"));
        }
        self.json(Method::GET, &path, None).await
    }

    pub async fn movie_by_tmdb(&self, tmdb: i64) -> Result<MovieDetail> {
        self.json(Method::GET, &format!("api/movie/tmdb/{tmdb}"), None)
            .await
    }

    /// Searches, waits for roarr's best pick to settle and grabs it.
    pub async fn auto(&self, request: &AutoRequest) -> Result<AutoResult> {
        let response = self
            .send(
                Method::POST,
                "api/auto",
                Some(serde_json::to_value(request)?),
                Some(AUTO_TIMEOUT),
            )
            .await?;
        Ok(response.json().await?)
    }

    async fn json<T: DeserializeOwned>(
        &self, method: Method, path: &str, body: Option<serde_json::Value>,
    ) -> Result<T> {
        Ok(self.send(method, path, body, None).await?.json().await?)
    }

    async fn send(
        &self, method: Method, path: &str, body: Option<serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<reqwest::Response> {
        let config = self
            .config
            .load_full()
            .ok_or_else(|| anyhow!("No roarr is set for this server"))?;

        if self.cookie.lock().unwrap().is_none() {
            self.login(&config).await?;
        }

        let mut response = self
            .request(&config, method.clone(), path, body.as_ref(), timeout)
            .await?;
        if response.status() == StatusCode::UNAUTHORIZED {
            self.login(&config).await?;
            response = self
                .request(&config, method, path, body.as_ref(), timeout)
                .await?;
        }
        check(response).await
    }

    async fn request(
        &self, config: &Config, method: Method, path: &str, body: Option<&serde_json::Value>,
        timeout: Option<Duration>,
    ) -> Result<reqwest::Response> {
        let mut request = self.api(method, config.base.join(path)?);
        if let Some(cookie) = self.cookie.lock().unwrap().clone() {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(timeout) = timeout {
            request = request.timeout(timeout);
        }
        Ok(request.send().await?)
    }

    /// A request to roarr's API. roarr refuses any call that is not a GET or
    /// HEAD without `X-Roarr: 1`, its guard against cross-site requests.
    fn api(&self, method: Method, url: Url) -> reqwest::RequestBuilder {
        self.client.request(method, url).header("X-Roarr", "1")
    }

    async fn login(&self, config: &Config) -> Result<()> {
        let response = self
            .api(Method::POST, config.base.join("api/login")?)
            .json(&json!({ "username": config.username, "password": config.password }))
            .send()
            .await?;
        let response = check(response).await?;

        // With AUTH_MODE=none roarr signs nobody in and sets no cookie.
        let cookie = response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .find(|value| value.starts_with("roarr_session="))
            .and_then(|value| value.split(';').next())
            .map(str::to_owned);
        *self.cookie.lock().unwrap() = Some(cookie.unwrap_or_default());
        Ok(())
    }
}

/// roarr answers errors as `{"error": "..."}`.
async fn check(response: reqwest::Response) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    #[derive(Deserialize)]
    struct Error {
        error: String,
    }
    match response.json::<Error>().await {
        Ok(error) => bail!("roarr: {}", error.error),
        Err(_) => bail!("roarr answered {status}"),
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub subtitle: String,
    pub release: String,
    pub poster: String,
    /// downloading | queued | paused | importing | warning | failed | stalled
    pub status: String,
    /// 0 to 1.
    pub progress: f64,
    pub size: i64,
    pub size_left: i64,
    pub speed: i64,
    pub eta: String,
    pub messages: Vec<String>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct Requests {
    pub items: Vec<RequestItem>,
    pub configured: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct RequestItem {
    pub id: i64,
    /// pending | approved | declined | failed | completed
    pub status: String,
    pub kind: String,
    pub title: String,
    pub year: i64,
    pub poster: String,
    pub tmdb_id: i64,
    pub tvdb_id: i64,
    pub seasons: Vec<i64>,
    pub requested_by: String,
    /// unknown | pending | processing | partial | available
    pub available: String,
    pub in_library: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct DiscoverItem {
    /// tv | movie
    pub kind: String,
    pub title: String,
    pub year: i64,
    pub poster: String,
    pub overview: String,
    pub tmdb_id: i64,
    pub tvdb_id: i64,
    pub in_library: bool,
    pub requested: bool,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SeriesDetail {
    pub id: i64,
    pub title: String,
    pub tvdb_id: i64,
    pub tmdb_id: i64,
    pub in_library: bool,
    pub seasons: Vec<SeasonDetail>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct SeasonDetail {
    pub number: i64,
    pub episodes: Vec<EpisodeDetail>,
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct EpisodeDetail {
    pub id: i64,
    pub season: i64,
    pub number: i64,
    pub aired: bool,
    pub has_file: bool,
    pub queued: bool,
}

impl EpisodeDetail {
    /// What roarr's Get picks up: aired, not on disk and not already coming.
    pub fn is_missing(&self) -> bool {
        self.aired && !self.has_file && !self.queued && self.season != 0
    }
}

impl SeriesDetail {
    pub fn missing(&self) -> usize {
        self.seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .filter(|e| e.is_missing())
            .count()
    }

    pub fn queued(&self) -> usize {
        self.seasons
            .iter()
            .flat_map(|s| &s.episodes)
            .filter(|e| e.queued)
            .count()
    }

    /// Seasons with something missing, as (season, missing episodes).
    pub fn missing_by_season(&self) -> Vec<(i64, usize)> {
        self.seasons
            .iter()
            .filter(|s| s.number != 0)
            .map(|s| (s.number, s.episodes.iter().filter(|e| e.is_missing()).count()))
            .filter(|(_, missing)| *missing > 0)
            .collect()
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct MovieDetail {
    pub id: i64,
    pub title: String,
    pub tmdb_id: i64,
    pub in_library: bool,
    pub has_file: bool,
    pub queued: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct AutoRequest {
    /// tv | movie
    pub kind: String,
    #[serde(skip_serializing_if = "is_zero")]
    pub tvdb_id: i64,
    #[serde(skip_serializing_if = "is_zero")]
    pub tmdb_id: i64,
    /// Empty: every missing aired episode.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub seasons: Vec<i64>,
    #[serde(rename = "movies", skip_serializing_if = "Vec::is_empty")]
    pub movie_tmdb: Vec<i64>,
}

fn is_zero(value: &i64) -> bool {
    *value == 0
}

impl AutoRequest {
    pub fn series(tvdb_id: i64, tmdb_id: i64, seasons: Vec<i64>) -> Self {
        Self {
            kind: "tv".into(),
            tvdb_id,
            tmdb_id,
            seasons,
            movie_tmdb: Vec::new(),
        }
    }

    pub fn movie(tmdb_id: i64) -> Self {
        Self {
            kind: "movie".into(),
            tmdb_id,
            movie_tmdb: vec![tmdb_id],
            ..Default::default()
        }
    }
}

#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct AutoResult {
    pub grabs: Vec<serde_json::Value>,
    pub message: String,
}

impl AutoResult {
    /// One line for a toast.
    pub fn summary(&self) -> String {
        if !self.message.is_empty() {
            return self.message.clone();
        }
        match self.grabs.len() {
            0 => gettextrs::gettext("Nothing to get"),
            n => gettextrs::ngettext("Getting 1 release", "Getting {} releases", n as u32)
                .replace("{}", &n.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// roarr's mock skips the CSRF guard, so only this test catches a lost header.
    #[test]
    fn api_requests_carry_the_roarr_header() {
        let client = RoarrClient::default();
        let url = Url::parse("http://roarr.test/api/login").unwrap();
        let request = client.api(Method::POST, url).build().unwrap();
        assert_eq!(request.headers().get("X-Roarr").unwrap(), "1");
    }

    /// Runs against a live roarr, e.g. its mock: `node web/mock/server.mjs`,
    /// then `ROARR_TEST_URL=http://localhost:3999 cargo test -p jox roarr -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn against_live_roarr() {
        let url = std::env::var("ROARR_TEST_URL").expect("ROARR_TEST_URL");
        let client = RoarrClient::default();
        client.configure(&Account {
            roarr_url: url,
            username: "test".into(),
            ..Default::default()
        });

        let activity = client.activity().await.unwrap();
        assert!(!activity.is_empty());
        assert!(activity.iter().all(|a| (0.0..=1.0).contains(&a.progress)));

        let discover = client.discover("dune").await.unwrap();
        assert!(discover.iter().any(|d| !d.in_library));

        let requests = client.requests().await.unwrap();
        println!("requests configured: {}", requests.configured);

        let series = client.series_by_tvdb(70001, None).await.unwrap();
        println!(
            "{}: {} missing, {} queued, by season {:?}",
            series.title,
            series.missing(),
            series.queued(),
            series.missing_by_season()
        );

        let movie = client.movie_by_tmdb(542178).await.unwrap();
        println!("{}: has file {}", movie.title, movie.has_file);

        let result = client
            .auto(&AutoRequest::series(series.tvdb_id, 0, vec![]))
            .await
            .unwrap();
        println!("auto: {}", result.summary());

        let err = client.request_action(1, "approve").await.unwrap_err();
        println!("request action error: {err}");
    }
}
