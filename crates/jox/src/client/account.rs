use anyhow::{
    Context,
    Result,
    anyhow,
};
use serde::{
    Deserialize,
    Serialize,
};
use url::Url;

use crate::ui::provider::descriptor::VecSerialize;

#[derive(Serialize, Deserialize, Clone, PartialEq, Default)]
pub struct Account {
    pub servername: String,
    pub server: String,
    pub username: String,
    pub password: String,
    pub port: String,
    pub user_id: String,
    pub access_token: String,
    /// roarr instance that manages this server's library, if any.
    #[serde(default)]
    pub roarr_url: String,
}

impl Account {
    pub fn url(&self) -> Result<Url> {
        build_url(&self.server, &self.port)
    }
}

pub(super) fn build_url(url_str: &str, port: &str) -> Result<Url> {
    let mut url = Url::parse(url_str)?;
    let port = port.parse::<u16>().context("Invalid server port")?;
    url.set_port(Some(port))
        .map_err(|_| anyhow!("Failed to set port"))?;
    Ok(url)
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub struct Accounts {
    pub accounts: Vec<Account>,
}

impl VecSerialize<Account> for Vec<Account> {
    fn to_string(&self) -> String {
        serde_json::to_string(&self).expect("Failed to serialize Vec<Descriptor>")
    }
}
