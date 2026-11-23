use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{ops::Deref, path::Path, sync::Arc, time::Duration};
#[derive(Clone)]
pub struct Client {
    inner: reqwest::blocking::Client,
    _relay: Option<Arc<crate::socks_relay::Relay>>,
    pub config: Config,
    pub proxy: Option<String>,
}
impl Deref for Client {
    type Target = reqwest::blocking::Client;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
#[derive(Default, Clone, Serialize, Deserialize)]
pub struct Config {
    pub proxy: Option<String>,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if let Some(proxy) = &self.proxy {
            let parsed = url::Url::parse(proxy)?;
            // HTTP CONNECT proxies also carry HTTPS destination traffic, including FFmpeg.
            ensure!(
                ["http", "socks5", "socks5h"].contains(&parsed.scheme())
                    && parsed.host_str().is_some(),
                "Use an HTTP or SOCKS5 proxy URL"
            );
            ensure!(
                parsed.query().is_none()
                    && parsed.fragment().is_none()
                    && ["", "/"].contains(&parsed.path()),
                "Proxy URL cannot contain a path, query or fragment"
            );
        }
        Ok(())
    }
    pub fn client(&self) -> Result<Client> {
        self.validate()?;
        let mut builder = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(30))
            .user_agent(concat!(
                "RoyMusic/",
                env!("CARGO_PKG_VERSION"),
                " (music player)"
            ));
        let relay = if self.proxy.as_ref().is_some_and(|p| p.starts_with("socks5")) {
            Some(Arc::new(crate::socks_relay::Relay::start(
                self.proxy.as_deref().unwrap(),
            )?))
        } else {
            None
        };
        let proxy = relay
            .as_ref()
            .map(|r| r.proxy.clone())
            .or_else(|| self.proxy.clone());
        if let Some(proxy) = &proxy {
            builder = builder.proxy(reqwest::Proxy::all(proxy)?);
        }
        Ok(Client {
            inner: builder.build()?,
            _relay: relay,
            config: self.clone(),
            proxy,
        })
    }
    pub fn load(root: &Path) -> Result<Self> {
        let value = crate::vault::load(&root.join("network.dpapi"))?;
        let config = if let Some(value) = value {
            serde_json::from_value(value)?
        } else {
            Self::default()
        };
        Ok(config)
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        self.validate()?;
        crate::vault::save(&root.join("network.dpapi"), &serde_json::to_value(self)?)?;
        Ok(())
    }
    pub fn status(&self) -> Value {
        let url = self
            .proxy
            .as_ref()
            .and_then(|p| url::Url::parse(p).ok())
            .map(|u| {
                format!(
                    "{}://{}:{}",
                    u.scheme(),
                    u.host_str().unwrap_or(""),
                    u.port_or_known_default()
                        .unwrap_or(if u.scheme() == "http" { 80 } else { 1080 })
                )
            });
        json!({"configured":self.proxy.is_some(),"proxy":url,"credentials_encrypted":true,"supported_schemes":["http","socks5","socks5h"]})
    }
}
