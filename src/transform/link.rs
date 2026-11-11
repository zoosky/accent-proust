//! A link's destination as upstream writes it into the tree: percent-encoded,
//! with an international host name in punycode.
//!
//! Upstream gets this from markdown-it, which runs `normalizeLink` on every
//! link, image, autolink and reference definition while it tokenizes. That is
//! three libraries, ported here as they are at the versions upstream pins, so
//! that the output is byte-for-byte theirs:
//!
//! - **markdown-it 15.0.1**, `MarkdownIt.prototype.normalizeLink`: which parts
//!   are encoded, and that the host goes to punycode only for `http:`,
//!   `https:`, `mailto:` and a URL with no protocol.
//! - **mdurl 2.1.0**, `parse`, `encode` and `format`: how a URL is split into
//!   parts (a trimmed-down port of Node's legacy `url.parse`), and which bytes
//!   are percent-encoded.
//! - **punycode.js 2.3.1**, `toASCII`: RFC 3492 for each label that holds a
//!   non-ASCII character, and nothing else. It is *not* IDNA: no case
//!   folding and no normalisation, so `BÜCHER.de` is `xn--BCHER-2pa.de`, where
//!   an IDNA library gives `xn--bcher-kva.de`. A crate would not match.
//!
//! All three are MIT-licensed; mdurl's parser derives from Node.js, also MIT.
//!
//! Upstream encodes while parsing, so its syntax tree holds the encoded form
//! and its formatter writes it back into the source. Here the tree keeps what
//! the author wrote, and the transform encodes. `DIVERGENCES.md` entry 17.

/// What [`encode`] leaves alone besides ASCII letters and digits: mdurl's
/// `encode.defaultChars`.
const KEEP: &[u8] = b";/?:@&=+$,-_.!~*'()#";

/// The protocols whose host name goes to punycode (`RECODE_HOSTNAME_FOR`).
/// Compared as written, case included, as upstream does.
const RECODE_HOSTNAME_FOR: [&str; 3] = ["http:", "https:", "mailto:"];

/// markdown-it's `normalizeLink`.
#[must_use]
pub fn normalize_link(url: &str) -> String {
    let mut parsed = Url::parse(url);
    if let Some(hostname) = parsed.hostname.as_deref().filter(|host| !host.is_empty())
        && parsed
            .protocol
            .as_deref()
            .is_none_or(|protocol| RECODE_HOSTNAME_FOR.contains(&protocol))
        && let Some(ascii) = to_ascii(hostname)
    {
        parsed.hostname = Some(ascii);
    }
    for part in [
        &mut parsed.auth,
        &mut parsed.hostname,
        &mut parsed.pathname,
        &mut parsed.search,
        &mut parsed.hash,
    ] {
        if let Some(value) = part.as_mut().filter(|value| !value.is_empty()) {
            *value = encode(value);
        }
    }
    parsed.format()
}

/// mdurl's `encode` with its defaults: percent-encode every byte but an ASCII
/// letter, a digit and [`KEEP`], and keep a `%` that already starts a valid
/// escape.
fn encode(text: &str) -> String {
    use std::fmt::Write as _;

    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while let Some(&byte) = bytes.get(index) {
        if byte == b'%'
            && let Some(escape) = text.get(index..index + 3)
            && escape.bytes().skip(1).all(|hex| hex.is_ascii_hexdigit())
        {
            out.push_str(escape);
            index += 3;
            continue;
        }
        if byte.is_ascii_alphanumeric() || KEEP.contains(&byte) {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
        index += 1;
    }
    out
}

/// The parts of a URL, as mdurl's `Url` holds them. [`None`] is JavaScript's
/// `null`; an empty string is kept apart from it only where upstream does.
#[derive(Debug, Default, PartialEq)]
struct Url {
    protocol: Option<String>,
    slashes: bool,
    auth: Option<String>,
    port: Option<String>,
    hostname: Option<String>,
    hash: Option<String>,
    search: Option<String>,
    pathname: Option<String>,
}

/// Where a host name ends at the latest (`hostEndingChars`).
const HOST_ENDING: &[u8] = b"/?#";

/// What cannot be in a host name (`nonHostChars`: `%/?;#`, then the
/// `autoEscape` set, which is `'`, `unwise` and `delims`).
const NON_HOST: &[u8] = b"%/?;#'{}|\\^`<>\" \r\n\t";

/// The protocols that take `//` (`slashedProtocol`), as written.
const SLASHED: [&str; 5] = ["http:", "https:", "ftp:", "gopher:", "file:"];

/// The longest host name kept (`hostnameMaxLen`), in UTF-16 units.
const HOSTNAME_MAX_LEN: usize = 255;

impl Url {
    /// mdurl's `parse(url, true)`, the only way `normalizeLink` calls it: two
    /// slashes always mean a host follows.
    fn parse(url: &str) -> Url {
        let mut out = Url::default();
        let mut rest = trim(url);

        // `protocolPattern`, `/^([a-z0-9.+-]+:)/i`.
        let scheme = rest
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'+' | b'-'))
            .count();
        let protocol = (scheme > 0 && rest.as_bytes().get(scheme) == Some(&b':'))
            .then(|| rest[..=scheme].to_string());
        if let Some(protocol) = &protocol {
            rest = &rest[protocol.len()..];
        }
        let hostless = protocol.as_deref() == Some("javascript:");

        let slashes = rest.starts_with("//");
        if slashes && !hostless {
            rest = &rest[2..];
            out.slashes = true;
        }

        let owned;
        if !hostless
            && (slashes
                || protocol
                    .as_deref()
                    .is_some_and(|protocol| !SLASHED.contains(&protocol)))
        {
            let host_end = first_of(rest, HOST_ENDING);
            let at = match host_end {
                None => rest.rfind('@'),
                Some(end) => rest[..=end.min(rest.len() - 1)].rfind('@'),
            };
            if let Some(at) = at {
                out.auth = Some(rest[..at].to_string());
                rest = &rest[at + 1..];
            }

            let mut host_end = first_of(rest, NON_HOST).unwrap_or(rest.len());
            if host_end > 0 && rest.as_bytes().get(host_end - 1) == Some(&b':') {
                host_end -= 1;
            }
            let host = &rest[..host_end];
            rest = &rest[host_end..];
            out.parse_host(host);
            let hostname = out.hostname.take().unwrap_or_default();

            let ipv6 = hostname.starts_with('[') && hostname.ends_with(']');
            let mut hostname = hostname;
            if !ipv6 && let Some((valid, not_host)) = split_host(&hostname) {
                if !not_host.is_empty() {
                    owned = format!("{not_host}{rest}");
                    rest = &owned;
                }
                hostname = valid;
            }
            if hostname.encode_utf16().count() > HOSTNAME_MAX_LEN {
                hostname = String::new();
            }
            if ipv6 && hostname.len() >= 2 {
                hostname = hostname[1..hostname.len() - 1].to_string();
            }
            out.hostname = Some(hostname);
        }

        if let Some(hash) = rest.find('#') {
            out.hash = Some(rest[hash..].to_string());
            rest = &rest[..hash];
        }
        if let Some(query) = rest.find('?') {
            out.search = Some(rest[query..].to_string());
            rest = &rest[..query];
        }
        if !rest.is_empty() {
            out.pathname = Some(rest.to_string());
        }
        out.protocol = protocol;
        out
    }

    /// mdurl's `parseHost`: a trailing `:digits` is the port.
    fn parse_host(&mut self, host: &str) {
        let digits = host.bytes().rev().take_while(u8::is_ascii_digit).count();
        let mut host = host;
        if host
            .len()
            .checked_sub(digits + 1)
            .and_then(|colon| host.as_bytes().get(colon))
            == Some(&b':')
        {
            if digits > 0 {
                self.port = Some(host[host.len() - digits..].to_string());
            }
            host = &host[..host.len() - digits - 1];
        }
        if !host.is_empty() {
            self.hostname = Some(host.to_string());
        }
    }

    /// mdurl's `format`.
    fn format(&self) -> String {
        let mut out = String::new();
        out.push_str(self.protocol.as_deref().unwrap_or_default());
        if self.slashes {
            out.push_str("//");
        }
        if let Some(auth) = self.auth.as_deref().filter(|auth| !auth.is_empty()) {
            out.push_str(auth);
            out.push('@');
        }
        match self.hostname.as_deref() {
            Some(hostname) if hostname.contains(':') => {
                out.push('[');
                out.push_str(hostname);
                out.push(']');
            }
            Some(hostname) => out.push_str(hostname),
            None => {}
        }
        if let Some(port) = self.port.as_deref().filter(|port| !port.is_empty()) {
            out.push(':');
            out.push_str(port);
        }
        for part in [&self.pathname, &self.search, &self.hash] {
            out.push_str(part.as_deref().unwrap_or_default());
        }
        out
    }
}

/// JavaScript's `String.prototype.trim`: Unicode white space and the byte
/// order mark, but not U+0085, which Unicode counts as white space and
/// JavaScript does not.
fn trim(text: &str) -> &str {
    text.trim_matches(|c: char| (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}')
}

/// The first offset of any of `bytes` in `text`.
fn first_of(text: &str, bytes: &[u8]) -> Option<usize> {
    text.bytes().position(|byte| bytes.contains(&byte))
}

/// Whether a label is a valid host name part (`hostnamePartPattern`,
/// `/^[+a-z0-9A-Z_-]{0,63}$/`). Length is in UTF-16 units, as in JavaScript.
fn is_host_part(label: &str) -> bool {
    label.encode_utf16().count() <= 63
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'_' | b'-'))
}

/// Where a host name stops being one: the valid labels, and the text that
/// goes back in front of the path. [`None`] when every label is valid.
///
/// mdurl lets a non-ASCII character through by testing a copy with each such
/// UTF-16 unit replaced by `x`. The first label that fails even so ends the
/// host at the longest valid prefix of it (`hostnamePartStart`).
fn split_host(hostname: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = hostname.split('.').collect();
    for (index, part) in parts.iter().enumerate() {
        if part.is_empty() || is_host_part(part) {
            continue;
        }
        let passes_as_ascii = part.encode_utf16().count() <= 63
            && part.chars().all(|c| {
                !c.is_ascii() || c.is_ascii_alphanumeric() || matches!(c, '+' | '_' | '-')
            });
        if passes_as_ascii {
            continue;
        }
        let owned = |slice: Option<&[&str]>| -> Vec<String> {
            slice
                .unwrap_or_default()
                .iter()
                .map(ToString::to_string)
                .collect()
        };
        let mut valid = owned(parts.get(..index));
        let mut not_host = owned(parts.get(index + 1..));
        // `/^([+a-z0-9A-Z_-]{0,63})(.*)$/`: `.` does not match a line
        // terminator, so a part holding one does not match at all.
        let lead: usize = part
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'_' | b'-'))
            .take(63)
            .count();
        let tail = &part[lead..];
        if !tail.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
            valid.push(part[..lead].to_string());
            not_host.insert(0, tail.to_string());
        }
        return Some((valid.join("."), not_host.join(".")));
    }
    None
}

/// punycode.js's `toASCII`, or [`None`] when encoding overflows, which
/// markdown-it catches by leaving the host name as it was.
fn to_ascii(domain: &str) -> Option<String> {
    let mut parts = domain.split('@');
    let first = parts.next().unwrap_or_default();
    let (prefix, domain) = match parts.next() {
        Some(domain) => (format!("{first}@"), domain),
        None => (String::new(), first),
    };
    let domain = domain.replace(['\u{3002}', '\u{ff0e}', '\u{ff61}'], ".");
    let mut labels = Vec::new();
    for label in domain.split('.') {
        if label.is_ascii() {
            labels.push(label.to_string());
        } else {
            labels.push(format!("xn--{}", punycode(label)?));
        }
    }
    Some(prefix + &labels.join("."))
}

/// RFC 3492 encoding, as punycode.js's `encode` does it.
fn punycode(input: &str) -> Option<String> {
    const BASE: u32 = 36;
    const T_MIN: u32 = 1;
    const T_MAX: u32 = 26;
    const MAX_INT: u32 = 0x7FFF_FFFF;

    let points: Vec<u32> = input.chars().map(u32::from).collect();
    let mut out: String = input.chars().filter(char::is_ascii).collect();
    let basic = u32::try_from(out.len()).ok()?;
    let mut handled = basic;
    if basic > 0 {
        out.push('-');
    }
    let total = u32::try_from(points.len()).ok()?;
    let (mut next, mut delta, mut bias) = (128_u32, 0_u32, 72_u32);
    while handled < total {
        let least = points
            .iter()
            .copied()
            .filter(|&point| point >= next)
            .min()?;
        let step = (least - next).checked_mul(handled + 1)?;
        if step > MAX_INT - delta {
            return None;
        }
        delta += step;
        next = least;
        for &point in &points {
            if point < next {
                delta = delta.checked_add(1).filter(|&delta| delta <= MAX_INT)?;
            }
            if point == next {
                let mut rest = delta;
                let mut position = BASE;
                loop {
                    let threshold = if position <= bias {
                        T_MIN
                    } else if position >= bias + T_MAX {
                        T_MAX
                    } else {
                        position - bias
                    };
                    if rest < threshold {
                        break;
                    }
                    out.push(digit(threshold + (rest - threshold) % (BASE - threshold)));
                    rest = (rest - threshold) / (BASE - threshold);
                    position += BASE;
                }
                out.push(digit(rest));
                bias = adapt(delta, handled + 1, handled == basic);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        next += 1;
    }
    Some(out)
}

/// A punycode digit: `a`-`z` for 0-25, `0`-`9` for 26-35.
fn digit(value: u32) -> char {
    let byte = u8::try_from(value).unwrap_or_default();
    char::from(if byte < 26 {
        b'a' + byte
    } else {
        b'0' + byte - 26
    })
}

/// RFC 3492's bias adaptation.
fn adapt(delta: u32, points: u32, first: bool) -> u32 {
    const BASE: u32 = 36;
    const T_MIN: u32 = 1;
    const T_MAX: u32 = 26;
    const SKEW: u32 = 38;
    const DAMP: u32 = 700;
    let mut delta = if first { delta / DAMP } else { delta >> 1 };
    delta += delta / points;
    let mut k = 0;
    while delta > ((BASE - T_MIN) * T_MAX) >> 1 {
        delta /= BASE - T_MIN;
        k += BASE;
    }
    k + (BASE - T_MIN + 1) * delta / (delta + SKEW)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each pair is upstream's output, taken from markdown-it 15.0.1.
    #[test]
    fn destinations_encode_as_upstream_encodes_them() {
        for (written, encoded) in [
            ("/a b", "/a%20b"),
            ("/a{b}", "/a%7Bb%7D"),
            ("/50%", "/50%25"),
            ("/%zz", "/%25zz"),
            ("/a%20b", "/a%20b"),
            ("/a\"b", "/a%22b"),
            ("/ä.png", "/%C3%A4.png"),
            ("/😀", "/%F0%9F%98%80"),
            (
                "https://x.ch/s?q=ä b#ü",
                "https://x.ch/s?q=%C3%A4%20b#%C3%BC",
            ),
            ("https://bücher.de/", "https://xn--bcher-kva.de/"),
            ("mailto:max@bücher.de", "mailto:max@xn--bcher-kva.de"),
            ("//bücher.de/x", "//xn--bcher-kva.de/x"),
            ("ftp://bücher.de/x", "ftp://b%C3%BCcher.de/x"),
            ("https://xn--bcher-kva.de/", "https://xn--bcher-kva.de/"),
            ("https://bücher.de/ä", "https://xn--bcher-kva.de/%C3%A4"),
            ("mailto:max@example.ch", "mailto:max@example.ch"),
            ("/x{%$y%}z", "/x%7B%25$y%25%7Dz"),
            ("https://BÜCHER.de/", "https://xn--BCHER-2pa.de/"),
            ("https://ｂücher．de/", "https://xn--cher-zra77610b.de/"),
            ("\u{85}/x", "%C2%85/x"),
            ("\u{feff} /x\u{a0}", "/x"),
        ] {
            assert_eq!(normalize_link(written), encoded, "{written:?}");
        }
    }

    #[test]
    fn punycode_matches_rfc_3492() {
        // RFC 3492 section 7.1, sample (A) Arabic and (L) Japanese.
        assert_eq!(
            punycode("\u{0644}\u{064a}\u{0647}\u{0645}\u{0627}\u{0628}\u{062a}\u{0643}\u{0644}\u{0645}\u{0648}\u{0634}\u{0639}\u{0631}\u{0628}\u{064a}\u{061f}").as_deref(),
            Some("egbpdaj6bu4bxfgehfvwxn")
        );
        assert_eq!(
            punycode("3年B組金八先生").as_deref(),
            Some("3B-ww4c5e180e575a65lsy2b")
        );
        assert_eq!(punycode("bücher").as_deref(), Some("bcher-kva"));
    }
}
