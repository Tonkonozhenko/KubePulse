//! Health read from status text, so the same word looks the same everywhere.
//! The text itself is always shown as received.

use gpui_kit::{
    SharedString,
    component::{Sizable as _, tag::Tag},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Success,
    Warning,
    Danger,
    Neutral,
}

const SUCCESS: &[&str] = &[
    "running",
    "succeeded",
    "completed",
    "active",
    "bound",
    "available",
    "ready",
    "healthy",
];

const WARNING: &[&str] = &[
    "pending",
    "containercreating",
    "podinitializing",
    "terminating",
    "released",
    "unknown",
];

const DANGER: &[&str] = &[
    "failed",
    "error",
    "crashloopbackoff",
    "imagepullbackoff",
    "errimagepull",
    "createcontainerconfigerror",
    "oomkilled",
    "evicted",
    "lost",
];

pub fn status_tone(status: &str) -> Tone {
    let status = status.to_ascii_lowercase();
    let listed = |list: &[&str]| list.contains(&status.as_str());
    if listed(SUCCESS) {
        Tone::Success
    } else if listed(WARNING) {
        Tone::Warning
    } else if listed(DANGER) {
        Tone::Danger
    } else {
        Tone::Neutral
    }
}

/// Some condition types name a problem, so True is the bad answer.
fn names_a_problem(kind: &str) -> bool {
    kind.ends_with("Pressure") || ["NetworkUnavailable", "ReplicaFailure", "Failed"].contains(&kind)
}

pub fn condition_tone(kind: &str, status: &str) -> Tone {
    match (status, names_a_problem(kind)) {
        ("True", true) => Tone::Danger,
        ("False", true) | ("True", false) => Tone::Success,
        // A False such as Ready on a finished pod is often expected.
        ("False", false) | ("Unknown", _) => Tone::Warning,
        _ => Tone::Neutral,
    }
}

pub fn event_tone(kind: &str) -> Tone {
    match kind {
        "Warning" => Tone::Warning,
        _ => Tone::Neutral,
    }
}

pub fn tone_tag(tone: Tone) -> Tag {
    match tone {
        Tone::Success => Tag::success(),
        Tone::Warning => Tag::warning(),
        Tone::Danger => Tag::danger(),
        Tone::Neutral => Tag::secondary(),
    }
    .small()
}

pub fn status_tag(text: impl Into<SharedString>) -> Tag {
    use gpui_kit::ParentElement as _;

    let text = text.into();
    tone_tag(status_tone(&text)).child(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_listed_status_has_its_tone() {
        for status in SUCCESS {
            assert_eq!(status_tone(status), Tone::Success, "{status}");
        }
        for status in WARNING {
            assert_eq!(status_tone(status), Tone::Warning, "{status}");
        }
        for status in DANGER {
            assert_eq!(status_tone(status), Tone::Danger, "{status}");
        }
    }

    #[test]
    fn statuses_match_in_any_case_and_unknown_words_stay_neutral() {
        assert_eq!(status_tone("CrashLoopBackOff"), Tone::Danger);
        assert_eq!(status_tone("RUNNING"), Tone::Success);
        assert_eq!(status_tone("Reticulating"), Tone::Neutral);
    }

    #[test]
    fn conditions_read_by_whether_their_type_names_a_problem() {
        assert_eq!(condition_tone("Ready", "True"), Tone::Success);
        assert_eq!(condition_tone("Ready", "False"), Tone::Warning);
        assert_eq!(condition_tone("DiskPressure", "False"), Tone::Success);
        assert_eq!(condition_tone("DiskPressure", "True"), Tone::Danger);
        assert_eq!(condition_tone("ReplicaFailure", "True"), Tone::Danger);
        assert_eq!(condition_tone("Ready", "Unknown"), Tone::Warning);
        assert_eq!(condition_tone("DiskPressure", "Unknown"), Tone::Warning);
    }

    #[test]
    fn warning_events_stand_out() {
        assert_eq!(event_tone("Warning"), Tone::Warning);
        assert_eq!(event_tone("Normal"), Tone::Neutral);
    }
}
