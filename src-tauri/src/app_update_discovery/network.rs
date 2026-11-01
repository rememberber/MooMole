use super::*;
use std::net::{IpAddr, SocketAddr};
pub(super) fn url(raw: &str) -> Result<reqwest::Url, String> {
    if raw.len() > 4096 || raw.trim() != raw || raw.chars().any(char::is_control) {
        return Err("更新地址格式无法核对。".into());
    }
    let u = reqwest::Url::parse(raw).map_err(|_| "更新地址无法解析。")?;
    let host = u.domain().ok_or("更新地址必须使用公开 HTTPS 域名。")?;
    if u.scheme() != "https"
        || !u.username().is_empty()
        || u.password().is_some()
        || u.port().is_some_and(|p| p != 443)
        || u.fragment().is_some()
        || !host.contains('.')
        || host.ends_with('.')
        || [
            "localhost",
            "local",
            "localdomain",
            "internal",
            "home",
            "test",
            "invalid",
            "example",
            "onion",
        ]
        .iter()
        .any(|v| host == *v || host.ends_with(&format!(".{v}")))
    {
        return Err("更新地址必须是无账户信息的公开 HTTPS 地址，且使用标准端口。".into());
    }
    Ok(u)
}
pub(super) fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => {
            let n = u32::from(a);
            ![
                (0u32, 8u32),
                (0x0a000000, 8),
                (0x64400000, 10),
                (0x7f000000, 8),
                (0xa9fe0000, 16),
                (0xac100000, 12),
                (0xc0000000, 24),
                (0xc0000200, 24),
                (0xc0586300, 24),
                (0xc0a80000, 16),
                (0xc6120000, 15),
                (0xc6336400, 24),
                (0xcb007100, 24),
                (0xe0000000, 3),
            ]
            .iter()
            .any(|(p, b)| n & (!0u32 << (32 - b)) == *p)
        }
        IpAddr::V6(a) => {
            let s = a.segments();
            s[0] & 0xe000 == 0x2000
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && s[0] != 0x2002
                && !(s[0] == 0x3fff && s[1] & 0xf000 == 0)
        }
    }
}
pub(super) fn resolve(u: &reqwest::Url) -> Result<Vec<SocketAddr>, String> {
    let host = u.domain().ok_or("更新域名无法核对。")?.to_string();
    // Called on our held blocking worker: a slow OS resolver keeps the slot,
    // registration and installation lease until it actually returns.
    let addresses = std::net::ToSocketAddrs::to_socket_addrs(&(host.as_str(), 443))
        .map_err(|_| "更新域名无法解析。")?
        .take(17)
        .collect::<Vec<_>>();
    if addresses.is_empty() || addresses.len() > 16 || addresses.iter().any(|a| !public(a.ip())) {
        return Err("更新域名指向非公开或无法核对的地址，未连接。".into());
    }
    Ok(addresses)
}
pub(super) async fn fetch(raw: &str, job: Arc<Job>) -> Result<Vec<u8>, String> {
    let u = url(raw)?;
    let stop = job.clone();
    let action = async move {
        let addresses = resolve(&u)?;
        check(&job)?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(u.domain().unwrap(), &addresses)
            .user_agent("MooMole/0.2 application-update-check")
            .build()
            .map_err(|_| "更新连接无法创建。")?;
        let mut response = client
            .get(u)
            .header("Accept", "application/rss+xml, application/xml, text/xml")
            .header("Accept-Encoding", "identity")
            .send()
            .await
            .map_err(|_| "更新来源连接失败或超时，版本状态未知。")?;
        if !response.status().is_success() {
            return Err("更新来源未返回成功响应；重定向不会自动跟随。".into());
        }
        if response.remote_addr().is_some_and(|a| !public(a.ip()))
            || response.content_length().is_some_and(|n| n > 1024 * 1024)
        {
            return Err("更新响应地址或容量不受支持。".into());
        }
        let mut bytes = vec![];
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "更新来源内容未完整读取。")?
        {
            check(&job)?;
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("更新来源内容超过 1 MiB 上限。".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    };
    tokio::pin!(action);
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    loop {
        tokio::select! {r=&mut action=>return r,_=tick.tick()=>check(&stop)?}
    }
}
