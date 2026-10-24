//! Checks and normalization of the values a rule holds: host names, CIDR
//! ranges, ports, program names. Pure functions shared by both editing modes.

use std::net::{Ipv4Addr, Ipv6Addr};

use super::advanced::CondKind;

/// Checks a host name such as `example.com` and returns its ASCII form, lowercased.
/// Unicode names (`кремль.рф`) are converted to punycode. A leading `*.` or `.`,
/// a trailing dot, a `scheme://` prefix and a path are dropped. A host needs at
/// least two labels and an alphabetic last label, so `1.2.3` is not a domain.
pub fn normalize_host(input: &str) -> Option<String> {
    let text = input.trim();
    let text = text.split_once("://").map_or(text, |(_, rest)| rest);
    let host = text.split('/').next().unwrap_or_default();
    let host = host.trim_start_matches("*.").trim_start_matches('.');
    let host = host.trim_end_matches('.').to_lowercase();
    let host = if host.is_ascii() {
        host
    } else {
        idna::domain_to_ascii(&host).ok()?
    };
    if host.len() > 253 {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 || !labels.iter().all(|label| is_host_label(label)) {
        return None;
    }
    let last = labels.last()?;
    last.chars()
        .any(|c| c.is_ascii_alphabetic())
        .then_some(host)
}

/// The Unicode form of an ASCII domain, for the screen. Other text is returned as is.
pub fn domain_for_display(ascii: &str) -> String {
    idna::domain_to_unicode(ascii).0
}

fn is_host_label(label: &str) -> bool {
    !label.is_empty()
        && label.len() <= 63
        && !label.starts_with('-')
        && !label.ends_with('-')
        && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// Parses an IP address or a CIDR range. A plain address becomes a host range
/// (`/32` or `/128`). Returns the normalized `address/prefix` text.
pub fn normalize_cidr(input: &str) -> Option<String> {
    let text = input.trim();
    let (address, prefix) = match text.split_once('/') {
        Some((address, prefix)) => (address, Some(prefix)),
        None => (text, None),
    };
    let (address, max) = if let Ok(v4) = address.parse::<Ipv4Addr>() {
        (v4.to_string(), 32)
    } else if let Ok(v6) = address.parse::<Ipv6Addr>() {
        (v6.to_string(), 128)
    } else {
        return None;
    };
    let prefix = match prefix {
        Some(digits) => digits.parse::<u8>().ok().filter(|prefix| *prefix <= max)?,
        None => max,
    };
    Some(format!("{address}/{prefix}"))
}

/// Whether a rule-set name looks like one of Throne's (`geosite-…`, `geoip-…`).
pub fn is_rule_set_name(text: &str) -> bool {
    let rest = text
        .strip_prefix("geosite-")
        .or_else(|| text.strip_prefix("geoip-"));
    rest.is_some_and(|rest| {
        !rest.is_empty()
            && text
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    })
}

/// Whether a program name is plausible: letters, digits and `-_+()`, and a dot
/// only before `.exe`. Names without a dot (`telegram`) are program names too.
pub fn is_process_name(text: &str) -> bool {
    let (stem, has_exe) = match strip_exe(text) {
        Some(stem) => (stem, true),
        None => (text, false),
    };
    !stem.is_empty()
        && stem
            .chars()
            .all(|c| c.is_alphanumeric() || "-_+()".contains(c))
        && (has_exe || !text.contains('.'))
}

/// The part of `text` before a case-insensitive `.exe` suffix.
fn strip_exe(text: &str) -> Option<&str> {
    let cut = text.len().checked_sub(4)?;
    let (stem, tail) = (text.get(..cut)?, text.get(cut..)?);
    tail.eq_ignore_ascii_case(".exe").then_some(stem)
}

/// A port from 1 to 65535.
pub fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|port| *port != 0)
}

/// A port range `start:end`. A dash is accepted too; the result uses `:`.
pub fn normalize_port_range(input: &str) -> Option<String> {
    let text = input.trim();
    let (start, end) = text.split_once(':').or_else(|| text.split_once('-'))?;
    let (start, end) = (parse_port(start)?, parse_port(end)?);
    (start <= end).then(|| format!("{start}:{end}"))
}

/// Checks a value of `kind` as the user typed it and returns what a rule stores:
/// hosts and keywords lowercased, CIDR ranges with a prefix, ports as numbers.
pub fn normalize_value(kind: CondKind, input: &str) -> Result<String, String> {
    let text = input.trim();
    match kind {
        CondKind::Domain | CondKind::DomainSuffix => {
            normalize_host(text).ok_or_else(|| format!("Не похоже на сайт: «{text}»"))
        }
        CondKind::DomainKeyword => non_empty(text).map(|keyword| keyword.to_lowercase()),
        CondKind::DomainRegex | CondKind::ProcessPath => non_empty(text),
        CondKind::IpCidr | CondKind::SourceIpCidr => {
            normalize_cidr(text).ok_or_else(|| format!("Не похоже на IP или подсеть: «{text}»"))
        }
        CondKind::Port => parse_port(text)
            .map(|port| port.to_string())
            .ok_or_else(|| "Порт — число от 1 до 65535".to_owned()),
        CondKind::PortRange => normalize_port_range(text).ok_or_else(|| {
            "Диапазон портов — например 1000:2000, начало не больше конца".to_owned()
        }),
        CondKind::ProcessName => {
            if is_process_name(text) {
                Ok(text.to_owned())
            } else {
                Err(format!("Не похоже на программу: «{text}»"))
            }
        }
        CondKind::RuleSet => {
            if text.is_empty() || text.contains(char::is_whitespace) {
                Err("Имя списка без пробелов, например geosite-ru".to_owned())
            } else {
                Ok(text.to_owned())
            }
        }
        CondKind::Network => {
            let lower = text.to_lowercase();
            if lower == "tcp" || lower == "udp" {
                Ok(lower)
            } else {
                Err("Сеть — tcp или udp".to_owned())
            }
        }
        CondKind::Protocol => {
            let lower = text.to_lowercase();
            if !lower.is_empty()
                && lower
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                Ok(lower)
            } else {
                Err("Протокол латиницей, например dns, tls или quic".to_owned())
            }
        }
        CondKind::IpIsPrivate | CondKind::Invert => {
            Err("Для этого условия выберите да или нет".to_owned())
        }
    }
}

fn non_empty(text: &str) -> Result<String, String> {
    if text.is_empty() {
        Err("Введите значение".to_owned())
    } else {
        Ok(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts_are_lowercased_and_stripped() {
        assert_eq!(
            normalize_host("YouTube.com").as_deref(),
            Some("youtube.com")
        );
        assert_eq!(
            normalize_host("*.Example.com.").as_deref(),
            Some("example.com")
        );
        assert_eq!(normalize_host(".example.ru").as_deref(), Some("example.ru"));
        assert_eq!(
            normalize_host("https://www.example.com/a/b").as_deref(),
            Some("www.example.com")
        );
    }

    #[test]
    fn unicode_hosts_become_punycode() {
        assert_eq!(
            normalize_host("кремль.рф").as_deref(),
            Some("xn--e1ajeds9e.xn--p1ai")
        );
        assert_eq!(
            normalize_host("Пример.Испытание").as_deref(),
            Some("xn--e1afmkfd.xn--80akhbyknj4f")
        );
        assert_eq!(domain_for_display("xn--e1ajeds9e.xn--p1ai"), "кремль.рф");
    }

    #[test]
    fn bad_hosts_are_rejected() {
        for text in [
            "",
            "ru",
            "1.2.3",
            "example.com:443",
            "a b.com",
            "-x.com",
            "x-.com",
            "a..b",
            "x_y.com",
            "a*b.com",
        ] {
            assert_eq!(normalize_host(text), None, "{text:?}");
        }
    }

    #[test]
    fn cidr_ranges_and_addresses() {
        assert_eq!(
            normalize_cidr("192.168.0.0/16").as_deref(),
            Some("192.168.0.0/16")
        );
        assert_eq!(normalize_cidr("1.2.3.4").as_deref(), Some("1.2.3.4/32"));
        assert_eq!(
            normalize_cidr("2001:db8::1").as_deref(),
            Some("2001:db8::1/128")
        );
        assert_eq!(
            normalize_cidr("2001:db8::/32").as_deref(),
            Some("2001:db8::/32")
        );
        assert_eq!(normalize_cidr("1.2.3.4/33"), None);
        assert_eq!(normalize_cidr("1.2.3.4/"), None);
        assert_eq!(normalize_cidr("example.com"), None);
    }

    #[test]
    fn program_names() {
        assert!(is_process_name("telegram"));
        assert!(is_process_name("steam.exe"));
        assert!(is_process_name("Steam.EXE"));
        assert!(is_process_name("cs2"));
        assert!(!is_process_name(""));
        assert!(!is_process_name(".exe"));
        assert!(!is_process_name("my.app"));
        assert!(!is_process_name("Google Chrome"));
        assert!(!is_process_name("a/b"));
    }

    #[test]
    fn port_ranges() {
        assert_eq!(
            normalize_port_range("27015-27050").as_deref(),
            Some("27015:27050")
        );
        assert_eq!(
            normalize_port_range("1000:2000").as_deref(),
            Some("1000:2000")
        );
        assert_eq!(normalize_port_range("2000:1000"), None);
        assert_eq!(normalize_port_range("0:10"), None);
        assert_eq!(normalize_port_range("443"), None);
    }

    #[test]
    fn ports_are_numbers_from_one() {
        assert_eq!(parse_port(" 443 "), Some(443));
        assert_eq!(parse_port("65535"), Some(65535));
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port("65536"), None);
        assert_eq!(parse_port("http"), None);
    }

    #[test]
    fn rule_set_names() {
        assert!(is_rule_set_name("geosite-ru"));
        assert!(is_rule_set_name("geoip-ru-blocked"));
        assert!(!is_rule_set_name("geosite-"));
        assert!(!is_rule_set_name("ru-geosite"));
        assert!(!is_rule_set_name("geosite-Ru"));
    }
}
