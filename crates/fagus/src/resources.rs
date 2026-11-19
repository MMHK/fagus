//! Where a document's resources come from. Fagus reads nothing on its own: `data:` URLs, the
//! resources a caller hands over, a caller's loader, and files inside the document's base
//! directory are all there is. There is no network access.

use std::path::Path;

/// Loads resources (stylesheets, images, fonts) for a document. Implement it to fetch URLs from
/// a database, a cache or the network, under your own rules.
pub trait ResourceLoader: Send + Sync {
    /// The data at `url`, exactly as written in the document (relative URLs included), or `None`
    /// if it is not available. The documents of frames (`<iframe src>`, `<object data>`) ask for
    /// their resources by their URLs resolved against the frame's URL, relative to the document
    /// (`sub/style.css` for `style.css` in `sub/page.html`).
    fn load(&self, url: &str) -> Option<Vec<u8>>;
}

/// The data of a `data:` URL (RFC 2397), base64 or percent-encoded.
pub(crate) fn data_url(url: &str) -> Option<Vec<u8>> {
    let rest = url.strip_prefix("data:").or_else(|| {
        url.get(..5)
            .filter(|scheme| scheme.eq_ignore_ascii_case("data:"))
            .map(|_| &url[5..])
    })?;
    let (header, data) = rest.split_once(',')?;
    if header
        .rsplit(';')
        .next()
        .is_some_and(|last| last.trim().eq_ignore_ascii_case("base64"))
    {
        base64(&percent_decode(data))
    } else {
        Some(percent_decode(data))
    }
}

fn percent_decode(input: &str) -> Vec<u8> {
    let bytes = input.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && let Some(hex) = input.get(index + 1..index + 3)
            && let Ok(value) = u8::from_str_radix(hex, 16)
        {
            output.push(value);
            index += 3;
        } else {
            output.push(bytes[index]);
            index += 1;
        }
    }
    output
}

/// Decodes base64 (standard or URL alphabet), ignoring white space; padding is optional.
fn base64(input: &[u8]) -> Option<Vec<u8>> {
    let mut output = Vec::with_capacity(input.len() * 3 / 4);
    let mut buffer = 0u32;
    let mut bits = 0;
    for &byte in input {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\t' | b'\n' | b'\r' | b'\x0c' => continue,
            _ => return None,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    Some(output)
}

/// Reads the file that a URL names, if it lies inside `root`: a relative URL resolves against
/// `base`, a path from the root (`/images/logo.png`) against `root`, as on a web server. URLs
/// with a scheme or a host (`//`) and anything that leads outside `root` (`..`, symbolic links)
/// are refused, also where percent-encoding hides them: an absolute path, a drive or a network
/// share (`%5C%5Chost%5Cshare` on Windows) is refused before the file system is asked about it.
pub(crate) fn read_inside(base: &Path, root: &Path, url: &str) -> Option<Vec<u8>> {
    let path = url.split(['?', '#']).next()?;
    if path.is_empty() || path.contains(':') || path.starts_with("//") || path.starts_with('\\') {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let decoded = String::from_utf8(percent_decode(path)).ok()?;
    let relative = decoded.strip_prefix('/').unwrap_or(&decoded);
    if relative.contains('\0')
        || Path::new(relative).has_root()
        || Path::new(relative)
            .components()
            .any(|component| matches!(component, std::path::Component::Prefix(_)))
    {
        return None;
    }
    let file = match decoded.strip_prefix('/') {
        Some(from_root) => root.join(from_root),
        None => base.join(&decoded),
    }
    .canonicalize()
    .ok()?;
    if !file.starts_with(&root) || !file.is_file() {
        return None;
    }
    std::fs::read(file).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_urls() {
        assert_eq!(
            data_url("data:text/plain;base64,SGFsbG8=").as_deref(),
            Some(&b"Hallo"[..])
        );
        assert_eq!(
            data_url("data:,Hallo%20Welt").as_deref(),
            Some(&b"Hallo Welt"[..])
        );
        assert_eq!(
            data_url("DATA:image/png;base64,AAEC").as_deref(),
            Some(&[0, 1, 2][..])
        );
        assert_eq!(data_url("https://example.com/a.png"), None);
        assert_eq!(data_url("data:;base64,@@"), None);
    }

    #[test]
    fn stays_inside_the_root() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests");
        let base = root.join("corpus");
        assert!(read_inside(&root, &root, "fonts/Inter-LICENSE.txt").is_some());
        assert!(read_inside(&root, &root, "fonts/Inter-LICENSE.txt?v=2").is_some());
        assert!(
            read_inside(&base, &root, "../fonts/Inter-LICENSE.txt").is_some(),
            "up, but inside the root"
        );
        assert!(
            read_inside(&base, &base, "../fonts/Inter-LICENSE.txt").is_none(),
            "outside the root"
        );
        assert!(read_inside(&root, &root, "../Cargo.toml").is_none(), "outside");
        assert!(
            read_inside(&base, &root, "/fonts/Inter-LICENSE.txt").is_some(),
            "from the root"
        );
        assert!(
            read_inside(&root, &root, "/etc/hosts").is_none(),
            "not inside the root"
        );
        assert!(
            read_inside(&root, &root, "//example.com/a.png").is_none(),
            "a host"
        );
        assert!(
            read_inside(&root, &root, "/../Cargo.toml").is_none(),
            "above the root"
        );
        assert!(read_inside(&root, &root, "file:///etc/hosts").is_none(), "scheme");
        assert!(read_inside(&root, &root, "fonts").is_none(), "directory");
        assert!(
            read_inside(&root, &root, "/%2Fetc/hosts").is_none(),
            "an absolute path, encoded"
        );
        assert!(
            read_inside(&root, &root, "/%2Ffonts/Inter-LICENSE.txt").is_none(),
            "an encoded slash makes no path from the root"
        );
        assert!(read_inside(&root, &root, "fonts/%00").is_none(), "NUL");
        #[cfg(windows)]
        for url in [
            "%5C%5Chost%5Cshare%5Cfile.png",
            "/%5C%5Chost%5Cshare%5Cfile.png",
            "%5Cfonts%5CInter-LICENSE.txt",
            "C%3A%5CWindows%5Cwin.ini",
            "/C%3A/Windows/win.ini",
        ] {
            assert!(read_inside(&root, &root, url).is_none(), "{url}");
        }
    }
}
