//! Reading meaning out of Homebrew's console output.
//!
//! Homebrew has no machine-readable progress channel, so the UI's progress
//! display is derived from the same lines a terminal user would see: `==>`
//! marks a new phase, and download progress is a bar ending in a percentage.

#[derive(Clone, Debug, PartialEq)]
pub enum Signal {
    /// A new stage began, e.g. "Downloading jq".
    Phase(String),
    /// Download progress, 0.0–100.0.
    Percent(f32),
    /// Homebrew is asking for a decision; it will block until answered, and we
    /// have no terminal to answer on.
    NeedsInput,
}

pub fn interpret(line: &str) -> Option<Signal> {
    let line = line.trim();

    if let Some(rest) = line.strip_prefix("==>") {
        let label = rest.trim();
        return (!label.is_empty()).then(|| Signal::Phase(label.to_owned()));
    }

    if let Some(percent) = percent_of(line) {
        return Some(Signal::Percent(percent));
    }

    if asks_for_input(line) {
        return Some(Signal::NeedsInput);
    }

    None
}

/// A progress bar looks like `####…####  62.5%`.
fn percent_of(line: &str) -> Option<f32> {
    let trimmed = line.trim_end();
    let value = trimmed.strip_suffix('%')?;
    let start = value
        .rfind(|c: char| !(c.is_ascii_digit() || c == '.'))
        .map_or(0, |i| i + 1);
    let number = &value[start..];
    if number.is_empty() {
        return None;
    }
    let percent: f32 = number.parse().ok()?;
    (0.0..=100.0).contains(&percent).then_some(percent)
}

/// Detect the prompts Homebrew emits when it wants a human. Without a terminal
/// these would hang forever, so the UI needs to surface them.
fn asks_for_input(line: &str) -> bool {
    let lowered = line.to_lowercase();
    lowered.contains("password:")
        || lowered.contains("press return")
        || lowered.contains("press enter")
        || (lowered.contains("[y/n]") || lowered.contains("(y/n)"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_phase_markers() {
        assert_eq!(
            interpret("==> Fetching jq"),
            Some(Signal::Phase("Fetching jq".into()))
        );
        assert_eq!(
            interpret("==> Pouring jq--1.8.2.arm64_tahoe.bottle.tar.gz"),
            Some(Signal::Phase(
                "Pouring jq--1.8.2.arm64_tahoe.bottle.tar.gz".into()
            ))
        );
        assert_eq!(
            interpret("==> Caveats"),
            Some(Signal::Phase("Caveats".into()))
        );
    }

    #[test]
    fn ignores_an_empty_phase_marker() {
        assert_eq!(interpret("==>"), None);
    }

    #[test]
    fn reads_download_percentages() {
        assert_eq!(interpret("#####  50.0%"), Some(Signal::Percent(50.0)));
        assert_eq!(
            interpret("######################## 100.0%"),
            Some(Signal::Percent(100.0))
        );
        assert_eq!(interpret("0.0%"), Some(Signal::Percent(0.0)));
    }

    #[test]
    fn rejects_percentages_that_are_not_progress() {
        assert_eq!(interpret("saved 400%"), None, "out of range");
        assert_eq!(interpret("%"), None);
    }

    #[test]
    fn detects_prompts_that_would_otherwise_hang() {
        assert_eq!(interpret("Password:"), Some(Signal::NeedsInput));
        assert_eq!(
            interpret("Press RETURN to continue or any other key to abort"),
            Some(Signal::NeedsInput)
        );
        assert_eq!(
            interpret("Do you want to proceed? [y/N]"),
            Some(Signal::NeedsInput)
        );
    }

    #[test]
    fn ordinary_output_carries_no_signal() {
        assert_eq!(interpret("Warning: jq 1.8.2 is already installed"), None);
        assert_eq!(interpret(""), None);
        assert_eq!(
            interpret("/opt/homebrew/Cellar/jq/1.8.2: 20 files, 1.2MB"),
            None
        );
    }
}
