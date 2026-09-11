use std::collections::HashMap;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SitePrivacy {
    pub shields: bool,
    pub block_third_party: bool,
    pub load_images: bool,
    pub javascript: bool,
}

impl Default for SitePrivacy {
    fn default() -> Self {
        Self {
            shields: true,
            block_third_party: false,
            load_images: true,
            javascript: true,
        }
    }
}

#[derive(Default)]
pub struct PrivacyProfiles {
    default: SitePrivacy,
    overrides: HashMap<String, SitePrivacy>,
}

impl PrivacyProfiles {
    pub fn new() -> Self {
        Self {
            default: SitePrivacy::default(),
            overrides: HashMap::new(),
        }
    }

    pub fn for_url(&self, url: &Url) -> SitePrivacy {
        url.host_str()
            .and_then(|host| self.overrides.get(&host.to_ascii_lowercase()).copied())
            .unwrap_or(self.default)
    }

    pub fn for_host(&self, host: &str) -> SitePrivacy {
        self.overrides
            .get(&host.to_ascii_lowercase())
            .copied()
            .unwrap_or(self.default)
    }

    pub fn set_for_host(&mut self, host: &str, value: SitePrivacy) {
        let host = host.to_ascii_lowercase();
        if value == self.default {
            self.overrides.remove(&host);
        } else {
            self.overrides.insert(host, value);
        }
    }

    pub fn clear_for_host(&mut self, host: &str) {
        self.overrides.remove(&host.to_ascii_lowercase());
    }

    pub fn default_settings(&self) -> SitePrivacy {
        self.default
    }
}

pub fn strip_tracking_parameters(mut url: Url) -> Url {
    let tracking_keys = [
        "utm_source",
        "utm_medium",
        "utm_campaign",
        "utm_term",
        "utm_content",
        "utm_id",
        "gclid",
        "dclid",
        "fbclid",
        "msclkid",
        "mc_cid",
        "mc_eid",
        "igshid",
        "yclid",
        "vero_conv",
        "vero_id",
        "oly_anon_id",
        "oly_enc_id",
        "wickedid",
        "twclid",
    ];

    let pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| {
            let lower = key.to_ascii_lowercase();
            !tracking_keys.contains(&lower.as_str()) && !lower.starts_with("utm_")
        })
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();

    if pairs.is_empty() {
        url.set_query(None);
        return url;
    }

    url.query_pairs_mut().clear().extend_pairs(pairs);
    url
}


pub fn site_key_for_url(url: &Url) -> String {
    url.host_str().map(|host| site_key(&host.to_ascii_lowercase())).unwrap_or_default()
}

pub fn is_third_party(top_level: &Url, request: &Url) -> bool {
    let top = top_level.host_str().unwrap_or_default().to_ascii_lowercase();
    let req = request.host_str().unwrap_or_default().to_ascii_lowercase();
    if top.is_empty() || req.is_empty() {
        return false;
    }
    site_key(&top) != site_key(&req)
}

pub fn site_key(host: &str) -> String {
    if host.parse::<std::net::IpAddr>().is_ok() || host == "localhost" {
        return host.to_owned();
    }

    let labels: Vec<&str> = host.split('.').filter(|part| !part.is_empty()).collect();
    if labels.len() <= 2 {
        return host.to_owned();
    }

    let last_two = format!("{}.{}", labels[labels.len() - 2], labels[labels.len() - 1]);
    let common_two_level_suffixes = [
        "co.uk", "org.uk", "ac.uk", "com.au", "net.au", "org.au", "co.nz",
        "co.jp", "ne.jp", "com.br", "com.cn", "com.sg", "com.tr", "co.in",
    ];

    if common_two_level_suffixes.contains(&last_two.as_str()) && labels.len() >= 3 {
        format!("{}.{}", labels[labels.len() - 3], last_two)
    } else {
        last_two
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sibling_subdomains_are_same_site() {
        let page = Url::parse("https://www.example.com/").unwrap();
        let image = Url::parse("https://cdn.example.com/a.png").unwrap();
        assert!(!is_third_party(&page, &image));
    }

    #[test]
    fn unrelated_domains_are_third_party() {
        let page = Url::parse("https://example.com/").unwrap();
        let image = Url::parse("https://tracker.example.net/p.gif").unwrap();
        assert!(is_third_party(&page, &image));
    }

    #[test]
    fn tracking_parameters_are_removed() {
        let url = Url::parse("https://example.com/page?q=ok&utm_source=x&fbclid=y").unwrap();
        let clean = strip_tracking_parameters(url);
        assert_eq!(clean.as_str(), "https://example.com/page?q=ok");
    }
}
