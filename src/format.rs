pub fn bytes(n: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = n as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else if value >= 100.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

/// A speed, or nothing at all when idle so quiet rows stay quiet.
pub fn speed(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        String::new()
    } else {
        format!("{}/s", bytes(bytes_per_second))
    }
}

pub fn eta(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return String::new();
    };
    let (d, h, m, s) = (
        seconds / 86_400,
        seconds / 3600 % 24,
        seconds / 60 % 60,
        seconds % 60,
    );
    if d > 99 {
        "∞".into()
    } else if d > 0 {
        format!("{d}d {h}h")
    } else if h > 0 {
        format!("{h}h {m:02}m")
    } else if m > 0 {
        format!("{m}m {s:02}s")
    } else {
        format!("{s}s")
    }
}

pub fn ratio(value: f32) -> String {
    if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    }
}

pub fn percent(fraction: f32) -> String {
    let value = fraction.clamp(0.0, 1.0) * 100.0;
    if value >= 99.95 {
        "100%".into()
    } else if value > 0.0 && value < 0.1 {
        "0.1%".into()
    } else {
        format!("{value:.1}%")
    }
}

pub fn count(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (ix, digit) in digits.chars().enumerate() {
        if ix > 0 && (digits.len() - ix).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

pub fn date(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .map(|at| {
            at.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_default()
}

/// Parses what people type for a speed limit: "500", "500k", "2.5 MB", "0" or blank for none. Bare numbers are KB/s.
pub fn parse_rate(text: &str) -> Option<u64> {
    let text = text.trim().to_ascii_lowercase().replace("/s", "");
    if text.is_empty() {
        return Some(0);
    }
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number.parse().ok()?;
    let scale = match unit.trim() {
        "" | "k" | "kb" | "kib" => 1024.0,
        "b" => 1.0,
        "m" | "mb" | "mib" => 1024.0 * 1024.0,
        "g" | "gb" | "gib" => 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    (number >= 0.0).then(|| (number * scale).round() as u64)
}

pub fn rate_input(bytes_per_second: u64) -> String {
    if bytes_per_second == 0 {
        String::new()
    } else if bytes_per_second.is_multiple_of(1024 * 1024) {
        format!("{} MB", bytes_per_second / (1024 * 1024))
    } else {
        format!("{} KB", bytes_per_second.div_ceil(1024))
    }
}

/// "HH:MM" to minutes after midnight.
pub fn parse_clock(text: &str) -> Option<u16> {
    let (h, m) = text.trim().split_once(':')?;
    let (h, m): (u16, u16) = (h.trim().parse().ok()?, m.trim().parse().ok()?);
    (h < 24 && m < 60).then_some(h * 60 + m)
}

pub fn clock(minutes: u16) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_bytes() {
        assert_eq!(bytes(0), "0 B");
        assert_eq!(bytes(1536), "1.5 KB");
        assert_eq!(bytes(250 * 1024 * 1024), "250 MB");
        assert_eq!(bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    #[test]
    fn idle_speeds_are_blank() {
        assert_eq!(speed(0), "");
        assert_eq!(speed(2048), "2.0 KB/s");
    }

    #[test]
    fn formats_eta() {
        assert_eq!(eta(None), "");
        assert_eq!(eta(Some(42)), "42s");
        assert_eq!(eta(Some(125)), "2m 05s");
        assert_eq!(eta(Some(3 * 3600 + 60)), "3h 01m");
        assert_eq!(eta(Some(2 * 86_400 + 3600)), "2d 1h");
        assert_eq!(eta(Some(500 * 86_400)), "∞");
    }

    #[test]
    fn formats_percent() {
        assert_eq!(percent(0.0), "0.0%");
        assert_eq!(percent(0.0004), "0.1%");
        assert_eq!(percent(0.4567), "45.7%");
        assert_eq!(percent(0.99999), "100%");
    }

    #[test]
    fn parses_rates() {
        assert_eq!(parse_rate(""), Some(0));
        assert_eq!(parse_rate("500"), Some(500 * 1024));
        assert_eq!(parse_rate("2.5 MB/s"), Some(2_621_440));
        assert_eq!(parse_rate("100b"), Some(100));
        assert_eq!(parse_rate("fast"), None);
        assert_eq!(rate_input(0), "");
        assert_eq!(rate_input(2 * 1024 * 1024), "2 MB");
        assert_eq!(rate_input(300 * 1024), "300 KB");
    }

    #[test]
    fn parses_clock() {
        assert_eq!(parse_clock("08:30"), Some(510));
        assert_eq!(parse_clock("24:00"), None);
        assert_eq!(clock(510), "08:30");
    }
}
