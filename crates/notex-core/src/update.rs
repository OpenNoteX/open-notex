//! Is there a newer release?
//!
//! The app asks GitHub for the latest release tag and compares it with the
//! version it was built with. Only the comparison lives here: the request is a
//! child process on the app's side, so this stays a pure function of two strings
//! and can be tested without a network, a clock or a window.
//!
//! Comparison is numeric, not textual. `0.10.0` is newer than `0.9.0`, which a
//! string compare gets backwards, and that mistake would hide every tenth minor
//! release from the person running the app.

use std::cmp::Ordering;
use std::fmt;

/// A release version: `major.minor.patch`, as the workspace writes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    /// Read `0.1.0`, `v0.1.0` or `0.1`. A trailing `.0` is assumed rather than
    /// required, because a release tag is written by a person.
    ///
    /// Anything this cannot order honestly is refused: pre-release suffixes
    /// (`1.0.0-rc1`) order *below* the release they precede, and a build
    /// suffix says nothing about which is newer.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        if text.is_empty() || !text.is_ascii() {
            return None;
        }

        let mut parts = text.split('.');
        let major = number(parts.next()?)?;
        let minor = match parts.next() {
            Some(part) => number(part)?,
            None => 0,
        };
        let patch = match parts.next() {
            Some(part) => number(part)?,
            None => 0,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
        })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

fn number(part: &str) -> Option<u64> {
    // `01` is not a version, it is a typo: accept it and a later `1.0.10` reads
    // as `1.0.010` to every tool that does not agree.
    if part.is_empty() || (part.len() > 1 && part.starts_with('0')) {
        return None;
    }
    part.parse().ok()
}

/// What a release tag means for the version this build reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    /// A release is newer than this build.
    Newer(Version),
    /// This build is the latest release.
    UpToDate,
    /// The tag is older than this build: a development build, or a release that
    /// was withdrawn. Nothing to offer.
    Older(Version),
    /// One of the two strings was not a version. Say nothing rather than guess.
    Unknown,
}

impl Availability {
    pub fn of(latest: &str, current: &str) -> Self {
        let (Some(latest), Some(current)) = (Version::parse(latest), Version::parse(current))
        else {
            return Availability::Unknown;
        };
        match latest.cmp(&current) {
            Ordering::Greater => Availability::Newer(latest),
            Ordering::Equal => Availability::UpToDate,
            Ordering::Less => Availability::Older(latest),
        }
    }

    /// The version to offer, when there is one.
    pub const fn offered(self) -> Option<Version> {
        match self {
            Availability::Newer(version) => Some(version),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} should parse"))
    }

    #[test]
    fn a_tag_is_read_with_or_without_its_v() {
        assert_eq!(version("0.1.0"), version("v0.1.0"));
        assert_eq!(version("V1.2.3"), version("1.2.3"));
        assert_eq!(version(" 0.1.0\n"), version("0.1.0"));
    }

    #[test]
    fn a_short_version_is_padded_rather_than_refused() {
        assert_eq!(version("0.1"), version("0.1.0"));
        assert_eq!(version("1"), version("1.0.0"));
    }

    #[test]
    fn a_pre_release_is_refused_because_it_orders_below_its_release() {
        assert_eq!(Version::parse("1.0.0-rc1"), None);
        assert_eq!(Version::parse("1.0.0+build.7"), None);
    }

    #[test]
    fn text_that_is_not_a_version_is_refused() {
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("latest"), None);
        assert_eq!(Version::parse("v"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("1..3"), None);
        assert_eq!(Version::parse("1.2.x"), None);
        assert_eq!(Version::parse("01.2.3"), None);
    }

    #[test]
    fn a_tenth_release_is_newer_than_a_ninth_one() {
        // The lexicographic mistake this module exists to prevent.
        assert!("0.10.0" < "0.9.0", "a string compare gets this wrong");
        assert_eq!(
            Availability::of("v0.10.0", "0.9.0"),
            Availability::Newer(version("0.10.0"))
        );
        assert_eq!(
            Availability::of("v0.9.0", "0.10.0"),
            Availability::Older(version("0.9.0"))
        );
    }

    #[test]
    fn the_same_version_offers_nothing() {
        assert_eq!(Availability::of("v0.1.0", "0.1.0"), Availability::UpToDate);
        assert_eq!(Availability::of("v0.1.0", "0.1.0").offered(), None);
    }

    #[test]
    fn a_newer_release_is_offered_by_its_number() {
        let availability = Availability::of("v0.1.0", "0.0.1");
        assert_eq!(availability, Availability::Newer(version("0.1.0")));
        assert_eq!(availability.offered().unwrap().to_string(), "0.1.0");
    }

    #[test]
    fn an_unreadable_tag_offers_nothing_rather_than_guessing() {
        assert_eq!(Availability::of("nightly", "0.0.1"), Availability::Unknown);
        assert_eq!(Availability::of("v0.1.0", "dev"), Availability::Unknown);
        assert_eq!(Availability::of("nightly", "0.0.1").offered(), None);
    }
}
