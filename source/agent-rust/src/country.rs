use crate::{Result, VERSION};
use std::{net::IpAddr, sync::Arc, time::Duration};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;
pub type Country = (String, String);
pub fn normalize(code: &str) -> Option<String> {
    let code = code.trim().to_ascii_uppercase();
    let code = if code == "UK" { "GB" } else { code.as_str() };
    if include_str!("country_codes.txt")
        .split_whitespace()
        .any(|v| v == code)
    {
        Some(code.into())
    } else {
        None
    }
}
pub fn parse_response(raw: &[u8], trace: bool) -> Result<Country> {
    if raw.len() > 8192 {
        return Err("country response too large");
    }
    let (mut code, mut ip) = (String::new(), String::new());
    if trace {
        for line in std::str::from_utf8(raw)
            .map_err(|_| "country response invalid")?
            .lines()
        {
            if let Some((k, v)) = line.split_once('=') {
                match k {
                    "loc" => code = v.into(),
                    "ip" => ip = v.into(),
                    _ => {}
                }
            }
        }
    } else {
        #[derive(serde::Deserialize)]
        struct Response {
            country: String,
            ip: String,
        }
        let v: Response = serde_json::from_slice(raw).map_err(|_| "country response invalid")?;
        code = v.country;
        ip = v.ip;
    }
    let country = normalize(&code).ok_or("country unavailable")?;
    let ip = ip
        .trim()
        .parse::<IpAddr>()
        .map_err(|_| "public address invalid")?;
    Ok((country, ip.to_string()))
}
async fn lookup(client: &reqwest::Client) -> Result<Country> {
    for (endpoint, trace) in [
        ("https://api.country.is/", false),
        ("https://www.cloudflare.com/cdn-cgi/trace", true),
    ] {
        let response = async {
            let mut res = client
                .get(endpoint)
                .send()
                .await
                .map_err(|_| "country lookup unavailable")?;
            if res.status() != reqwest::StatusCode::OK
                || res.content_length().is_some_and(|n| n > 8192)
            {
                return Err("country response invalid");
            }
            let mut raw = Vec::new();
            while let Some(chunk) = res
                .chunk()
                .await
                .map_err(|_| "country lookup unavailable")?
            {
                if raw.len() + chunk.len() > 8192 {
                    return Err("country response too large");
                }
                raw.extend_from_slice(&chunk);
            }
            parse_response(&raw, trace)
        };
        if let Ok(Ok(value)) = tokio::time::timeout(Duration::from_secs(6), response).await {
            return Ok(value);
        }
    }
    Err("country lookup unavailable")
}
pub async fn run(
    tls: Arc<rustls::ClientConfig>,
    sender: watch::Sender<Country>,
    cancel: CancellationToken,
) {
    let Ok(client) = reqwest::Client::builder()
        .tls_backend_preconfigured((*tls).clone())
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(4))
        .timeout(Duration::from_secs(6))
        .pool_max_idle_per_host(0)
        .user_agent(format!("Vistart-Probe/{VERSION}"))
        .build()
    else {
        return;
    };
    loop {
        let result = tokio::select! {_=cancel.cancelled()=>return,r=lookup(&client)=>r};
        let delay = if let Ok(value) = result {
            sender.send_replace(value);
            Duration::from_secs(86400)
        } else {
            Duration::from_secs(600)
        };
        tokio::select! {_=cancel.cancelled()=>return,_=tokio::time::sleep(delay)=>{}}
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn all_countries_and_alias() {
        assert_eq!(
            include_str!("country_codes.txt").split_whitespace().count(),
            250
        );
        assert_eq!(normalize(" uk "), Some("GB".into()));
        for v in ["SG", "IS", "CW", "XK", "AQ"] {
            assert_eq!(normalize(v), Some(v.into()));
        }
        assert_eq!(normalize("ZZ"), None);
    }
    #[test]
    fn validates_country_and_public_address() {
        assert_eq!(
            parse_response(br#"{"country":"sg","ip":"203.0.113.8"}"#, false).unwrap(),
            ("SG".into(), "203.0.113.8".into())
        );
        assert!(parse_response(br#"{"country":"SG","ip":"bad"}"#, false).is_err());
        assert!(parse_response(br#"{"country":"ZZ","ip":"203.0.113.8"}"#, false).is_err());
        assert_eq!(
            parse_response(b"loc=SG\nip=2001:db8::1\n", true).unwrap().1,
            "2001:db8::1"
        );
        assert!(parse_response(&vec![0; 8193], false).is_err());
    }
}
