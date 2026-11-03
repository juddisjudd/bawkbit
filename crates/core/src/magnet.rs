/// The info hash and display name in a magnet link or a bare info hash, enough to show a row before the metadata arrives.
#[derive(Debug, PartialEq, Eq)]
pub struct MagnetInfo {
    pub id: String,
    pub name: Option<String>,
}

pub fn parse(link: &str) -> Option<MagnetInfo> {
    let link = link.trim();
    if let Some(id) = hash_to_hex(link) {
        return Some(MagnetInfo { id, name: None });
    }
    let query = link.strip_prefix("magnet:?")?;
    let mut id = None;
    let mut name = None;
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        match key {
            "xt" => {
                if let Some(hash) = value
                    .strip_prefix("urn:btih:")
                    .or_else(|| value.strip_prefix("urn%3Abtih%3A"))
                {
                    id = id.or_else(|| hash_to_hex(hash));
                }
            }
            "dn" => name = Some(percent_decode(value)),
            _ => {}
        }
    }
    Some(MagnetInfo { id: id?, name })
}

fn hash_to_hex(hash: &str) -> Option<String> {
    if hash.len() == 40 && hash.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Some(hash.to_ascii_lowercase());
    }
    if hash.len() == 32 {
        let bytes = base32(hash)?;
        return Some(bytes.iter().map(|b| format!("{b:02x}")).collect());
    }
    None
}

fn base32(text: &str) -> Option<Vec<u8>> {
    let mut bits = 0u64;
    let mut count = 0;
    let mut out = Vec::with_capacity(20);
    for c in text.bytes() {
        let value = match c.to_ascii_uppercase() {
            c @ b'A'..=b'Z' => c - b'A',
            c @ b'2'..=b'7' => c - b'2' + 26,
            _ => return None,
        };
        bits = (bits << 5) | u64::from(value);
        count += 5;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
        }
    }
    (out.len() == 20).then_some(out)
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut ix = 0;
    while ix < bytes.len() {
        match bytes[ix] {
            b'+' => out.push(b' '),
            b'%' if ix + 2 < bytes.len() => {
                match (hex_digit(bytes[ix + 1]), hex_digit(bytes[ix + 2])) {
                    (Some(high), Some(low)) => {
                        out.push(high << 4 | low);
                        ix += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            byte => out.push(byte),
        }
        ix += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_digit(byte: u8) -> Option<u8> {
    (byte as char).to_digit(16).map(|digit| digit as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hex_magnets() {
        let info = parse("magnet:?xt=urn:btih:DD8255ECDC7CA55FB0BBF81323D87062DB1F6D1C&dn=Big+Buck%20Bunny&tr=udp%3A%2F%2Fx").unwrap();
        assert_eq!(info.id, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
        assert_eq!(info.name.as_deref(), Some("Big Buck Bunny"));
    }

    #[test]
    fn reads_base32_magnets() {
        let info = parse("magnet:?xt=urn:btih:3WBFL3G4PSSV7MF37AJSHWDQMLNR63I4").unwrap();
        assert_eq!(info.id, "dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c");
    }

    #[test]
    fn reads_bare_hashes() {
        let info = parse("dd8255ecdc7ca55fb0bbf81323d87062db1f6d1c").unwrap();
        assert_eq!(info.name, None);
        assert!(parse("https://example.com/a.torrent").is_none());
    }
}
