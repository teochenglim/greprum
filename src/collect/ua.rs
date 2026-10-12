//! Tiny user-agent classifier. No dependency: coarse buckets are all the
//! "Browsers and devices" screen needs.

pub struct Ua {
    pub device: &'static str,
    pub browser: &'static str,
    pub os: &'static str,
    pub bot: bool,
}

pub fn parse(ua: &str) -> Ua {
    let l = ua.to_ascii_lowercase();
    let bot = l.is_empty()
        || [
            "bot",
            "crawler",
            "spider",
            "headless",
            "slurp",
            "curl/",
            "wget/",
            "python-requests",
            "preview",
        ]
        .iter()
        .any(|m| l.contains(m));
    let device = if l.contains("ipad") || l.contains("tablet") {
        "tablet"
    } else if l.contains("mobi") || l.contains("iphone") || l.contains("android") {
        "mobile"
    } else {
        "desktop"
    };
    // Order matters: Edge/Opera/Chrome UAs all contain "chrome"/"safari".
    let browser = if l.contains("edg/") || l.contains("edge/") {
        "Edge"
    } else if l.contains("opr/") || l.contains("opera") {
        "Opera"
    } else if l.contains("firefox/") || l.contains("fxios") {
        "Firefox"
    } else if l.contains("chrome/") || l.contains("crios") {
        "Chrome"
    } else if l.contains("safari/") {
        "Safari"
    } else {
        "Other"
    };
    let os = if l.contains("windows") {
        "Windows"
    } else if l.contains("iphone") || l.contains("ipad") || l.contains("ios") {
        "iOS"
    } else if l.contains("android") {
        "Android"
    } else if l.contains("mac os") || l.contains("macintosh") {
        "macOS"
    } else if l.contains("linux") || l.contains("x11") {
        "Linux"
    } else {
        "Other"
    };
    Ua {
        device,
        browser,
        os,
        bot,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_common_agents() {
        let c = parse(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Safari/605.1.15",
        );
        assert_eq!(
            (c.browser, c.os, c.device, c.bot),
            ("Safari", "macOS", "desktop", false)
        );
        let c = parse(
            "Mozilla/5.0 (iPhone; CPU iPhone OS 17_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.0 Mobile/15E148 Safari/604.1",
        );
        assert_eq!((c.os, c.device), ("iOS", "mobile"));
        let c = parse(
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36 Edg/120.0",
        );
        assert_eq!((c.browser, c.os), ("Edge", "Windows"));
        assert!(parse("Googlebot/2.1 (+http://www.google.com/bot.html)").bot);
        assert!(parse("").bot);
    }
}
