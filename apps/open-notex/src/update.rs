//! The update check: one child process, one thread, one answer.
//!
//! A local-first app should not phone home on launch, so nothing here runs until
//! someone presses "Check now" in Settings. Then the app asks `curl` — which is
//! on every macOS — for GitHub's latest release and reads the tag out of the
//! answer. The decision itself is [`notex_core::update`], which is a pure
//! function of two strings, so "is 0.10 newer than 0.9" is tested without a
//! network, a clock or a window.

use std::process::Command;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver};

use notex_core::Availability;

/// GitHub's latest release. Drafts are not in it, so an unpublished artifact is
/// never offered to anybody.
pub const LATEST_RELEASE: &str =
    "https://api.github.com/repos/OpenNoteX/open-notex/releases/latest";

/// Where a release page lives. The app opens it in the browser rather than
/// downloading anything: installing an update is M6c.
pub const RELEASE_PAGE: &str = "https://github.com/OpenNoteX/open-notex/releases/tag/v";

/// A blocked or slow network must not leave a row saying "Checking…" forever.
const TIMEOUT_SECONDS: &str = "8";

/// The answer to one check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Checked {
    /// The tag GitHub reports, compared with the version this build reports.
    Compared(Availability),
    /// No answer, in a sentence that reads like one.
    Failed(String),
}

/// Ask GitHub on a thread of its own; `poll` picks the answer up.
pub struct Updater {
    results: Receiver<Checked>,
}

impl Updater {
    /// Start the one request. `notify` wakes the UI, which otherwise draws
    /// nothing until the next input and would show a stale row.
    pub fn start(current: &'static str, notify: impl Fn() + Send + Sync + 'static) -> Self {
        let (sender, results) = mpsc::channel();
        let spare = sender.clone();
        let notify = Arc::new(notify);
        let wake = Arc::clone(&notify);
        let spawned = std::thread::Builder::new()
            .name("update-check".to_owned())
            .spawn(move || {
                let _ = sender.send(check(current));
                wake();
            });
        if let Err(error) = spawned {
            // A machine that refuses the thread still gets an answer, rather than
            // a row stuck on "Checking…".
            let _ = spare.send(Checked::Failed(format!(
                "Could not start the update check: {error}"
            )));
            notify();
        }
        Self { results }
    }

    /// The answer, once. `None` while the check is still running.
    pub fn poll(&self) -> Option<Checked> {
        self.results.try_recv().ok()
    }
}

fn check(current: &str) -> Checked {
    match latest_tag(LATEST_RELEASE) {
        Ok(tag) => Checked::Compared(Availability::of(&tag, current)),
        Err(error) => Checked::Failed(error),
    }
}

/// The tag of the latest release, as GitHub wrote it.
fn latest_tag(url: &str) -> Result<String, String> {
    let output = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-time",
            TIMEOUT_SECONDS,
            "-H",
            "Accept: application/vnd.github+json",
            url,
        ])
        .output()
        .map_err(|error| format!("Could not run curl: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stderr = stderr.trim();
        return Err(if stderr.is_empty() {
            "GitHub did not answer.".to_owned()
        } else {
            format!("GitHub did not answer: {stderr}")
        });
    }
    tag_from_json(&output.stdout)
}

/// The `tag_name` of a release object. Kept apart from the request so the parsing
/// is tested against real answers instead of the network.
pub fn tag_from_json(body: &[u8]) -> Result<String, String> {
    let value: serde_json::Value = serde_json::from_slice(body)
        .map_err(|error| format!("GitHub's answer was not JSON: {error}"))?;
    match value.get("tag_name").and_then(serde_json::Value::as_str) {
        Some(tag) if !tag.trim().is_empty() => Ok(tag.trim().to_owned()),
        // A release always has one. If it does not, the answer is not one we
        // should act on.
        _ => Err("GitHub's answer did not name a release.".to_owned()),
    }
}

/// The page a version's notes and download live on.
pub fn release_page(version: &str) -> String {
    format!("{RELEASE_PAGE}{version}")
}

/// Open a URL in the browser. Best effort: a machine with no browser is not a
/// reason to fail the check, and the version is still on screen.
pub fn open_page(url: &str) {
    if let Err(error) = Command::new("open").arg(url).status() {
        eprintln!("openNoteX: could not open {url}: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The shape of the answer, with the fields a release has that we ignore.
    fn release(tag: &str) -> String {
        format!(
            r#"{{"url":"https://api.github.com/repos/x/y/releases/1","tag_name":"{tag}","name":"openNoteX {tag}","draft":false,"prerelease":false,"assets":[]}}"#
        )
    }

    #[test]
    fn the_tag_is_read_out_of_a_release() {
        assert_eq!(
            tag_from_json(release("v0.1.0").as_bytes()),
            Ok("v0.1.0".to_owned())
        );
    }

    #[test]
    fn an_answer_without_a_tag_is_refused() {
        let error = tag_from_json(br#"{"name":"openNoteX 0.1.0"}"#).unwrap_err();
        assert!(error.contains("did not name a release"), "{error}");
        let error = tag_from_json(br#"{"tag_name": 7}"#).unwrap_err();
        assert!(error.contains("did not name a release"), "{error}");
        let error = tag_from_json(br#"{"tag_name":"  "}"#).unwrap_err();
        assert!(error.contains("did not name a release"), "{error}");
    }

    #[test]
    fn an_answer_that_is_not_json_says_so() {
        let error = tag_from_json(b"<html>502 Bad Gateway</html>").unwrap_err();
        assert!(error.contains("was not JSON"), "{error}");
        let error = tag_from_json(b"").unwrap_err();
        assert!(error.contains("was not JSON"), "{error}");
    }

    #[test]
    fn a_release_page_is_the_tag_with_its_v() {
        assert_eq!(
            release_page("0.1.0"),
            "https://github.com/OpenNoteX/open-notex/releases/tag/v0.1.0"
        );
    }

    #[test]
    fn the_version_this_build_reports_is_compared_rather_than_matched() {
        let compared = |tag: &str| match tag_from_json(release(tag).as_bytes()) {
            Ok(tag) => Availability::of(&tag, "0.0.1"),
            Err(error) => panic!("{error}"),
        };
        assert_eq!(
            compared("v0.1.0"),
            Availability::Newer(notex_core::Version::parse("0.1.0").unwrap())
        );
        assert_eq!(compared("v0.0.1"), Availability::UpToDate);
        assert_eq!(
            compared("v0.0.0"),
            Availability::Older(notex_core::Version::parse("0.0.0").unwrap())
        );
    }
}
