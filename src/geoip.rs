//! Optional country lookup from a MaxMind-format database (R-G19).
//!
//! The client IP is read, looked up and dropped: it is never stored, spooled or logged.

use std::net::IpAddr;

pub struct GeoIp {
    reader: maxminddb::Reader<Vec<u8>>,
}

impl GeoIp {
    pub fn open(path: &str) -> anyhow::Result<GeoIp> {
        let bytes = std::fs::read(path)?;
        Ok(GeoIp {
            reader: maxminddb::Reader::from_source(bytes)?,
        })
    }

    /// ISO 3166-1 alpha-2 code, or empty for unknown/private addresses.
    pub fn country(&self, ip: IpAddr) -> String {
        if is_private(ip) {
            return String::new();
        }
        let Ok(res) = self.reader.lookup(ip) else {
            return String::new();
        };
        res.decode::<maxminddb::geoip2::Country>()
            .ok()
            .flatten()
            .and_then(|c| c.country.iso_code.map(str::to_string))
            .unwrap_or_default()
    }
}

pub fn is_private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => {
            v.is_private() || v.is_loopback() || v.is_link_local() || v.is_unspecified()
        }
        IpAddr::V6(v) => {
            v.is_loopback() || v.is_unspecified() || (v.segments()[0] & 0xfe00) == 0xfc00
        }
    }
}

/// First address of an `X-Forwarded-For`-style header.
pub fn first_ip(header: &str) -> Option<IpAddr> {
    header.split(',').next()?.trim().parse().ok()
}
