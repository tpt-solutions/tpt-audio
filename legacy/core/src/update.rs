//! Version comparison and remote update checking.
//!
//! The remote check is intentionally dependency-free: it shells out to `curl` (present
//! on Windows 10+, most Linux distros, and macOS) and performs a tiny, safe substring
//! extraction of the `tag_name` field. It degrades gracefully when `curl` is absent or
//! the network is unavailable.

use std::process::Command;

/// A semantic-ish version (`MAJOR.MINOR.PATCH`), optionally with a pre-release suffix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
    pub pre: String,
}

impl Version {
    /// Parse a version string like `1.2.3` or `1.2.3-beta.1`.
    /// Non-numeric components are tolerated (the whole trailing part becomes `pre`).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim().trim_start_matches('v');
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, format!("-{p}")),
            None => (s, String::new()),
        };
        let mut parts = core.split('.');
        let major = parts.next()?.parse().ok()?;
        let minor = parts.next().unwrap_or("0").parse().unwrap_or(0);
        let patch = parts.next().unwrap_or("0").parse().unwrap_or(0);
        Some(Version {
            major,
            minor,
            patch,
            pre,
        })
    }

    /// `true` if `self` is older than `other`.
    pub fn is_older_than(&self, other: &Version) -> bool {
        (self.major, self.minor, self.patch, self.pre.is_empty())
            < (other.major, other.minor, other.patch, other.pre.is_empty())
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}{}", self.major, self.minor, self.patch, self.pre)
    }
}

/// Query a GitHub "latest release" API URL and extract its `tag_name`.
/// Returns `None` on any failure (no network, no curl, unexpected payload).
pub fn latest_release_tag(api_url: &str) -> Option<String> {
    let output = Command::new("curl")
        .args(["-sS", "--max-time", "10", "-H", "Accept: application/vnd.github+json", api_url])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let body = String::from_utf8_lossy(&output.stdout);
    extract_tag_name(&body)
}

fn extract_tag_name(body: &str) -> Option<String> {
    let marker = "\"tag_name\"";
    let start = body.find(marker)? + marker.len();
    let rest = &body[start..];
    let colon = rest.find(':')? + 1;
    let rest = rest[colon..].trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    let tag = rest[..end].to_string();
    if tag.is_empty() {
        None
    } else {
        Some(tag)
    }
}

/// Compare `current` against the latest remote tag. Returns a human-readable status.
pub fn update_status(current: &str, api_url: &str) -> String {
    let cur = match Version::parse(current) {
        Some(v) => v,
        None => return "Installed version is unparsable; cannot check for updates.".to_string(),
    };
    match latest_release_tag(api_url) {
        None => "Could not reach the update server. Check your connection and try again.".to_string(),
        Some(tag) => match Version::parse(&tag) {
            Some(remote) if cur.is_older_than(&remote) => {
                format!("Update available: {cur} → {remote}")
            }
            Some(_) => format!("You are on the latest version ({cur})."),
            None => format!("Update server returned an unparsable version: {tag}"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_basic() {
        let v = Version::parse("1.2.3").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (1, 2, 3));
        assert!(v.pre.is_empty());
    }

    #[test]
    fn parse_with_prefix_and_pre() {
        let v = Version::parse("v2.0.1-rc.1").unwrap();
        assert_eq!((v.major, v.minor, v.patch), (2, 0, 1));
        assert_eq!(v.pre, "-rc.1");
    }

    #[test]
    fn ordering() {
        let a = Version::parse("1.2.3").unwrap();
        let b = Version::parse("1.2.4").unwrap();
        let c = Version::parse("2.0.0").unwrap();
        assert!(a.is_older_than(&b));
        assert!(a.is_older_than(&c));
        assert!(!b.is_older_than(&a));
    }

    #[test]
    fn extract() {
        let body = r#"{"tag_name":"v1.4.2","name":"release"}"#;
        assert_eq!(extract_tag_name(body).as_deref(), Some("v1.4.2"));
    }
}
