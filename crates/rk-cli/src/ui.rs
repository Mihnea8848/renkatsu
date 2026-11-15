// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: GPL-3.0-or-later

//! Terminal output in the style of cargo: a right-aligned coloured verb,
//! progress bars for long runs, and numbers coloured by how far they are
//! from the game. Colours and bars switch off on their own when the output
//! is not a terminal, so logs and pipes stay plain text.

use console::{style, StyledObject};
use indicatif::{ProgressBar, ProgressStyle};
use std::fmt::Display;
use std::time::Duration;

/// Width of the verb column, as cargo uses.
const VERB: usize = 12;

/// A status line: `   Comparing Simple (25 cards)`.
pub fn status(verb: &str, msg: impl Display) {
    println!("{} {msg}", style(format!("{verb:>VERB$}")).green().bold());
}

/// A failure line, red.
pub fn fail(msg: impl Display) {
    println!("{} {msg}", style(format!("{:>VERB$}", "error")).red().bold());
}

/// A section heading.
pub fn heading(text: impl Display) {
    println!("\n{}", style(text).bold().underlined());
}

/// A progress bar with a verb, count, elapsed time and time left. Lines
/// printed through `bar.println` appear above it.
pub fn bar(verb: &str, len: u64) -> ProgressBar {
    let bar = ProgressBar::new(len);
    let template = format!(
        "{{prefix:>{VERB}.cyan.bold}} [{{bar:32.cyan/blue}}] {{pos}}/{{len}} {{elapsed_precise}} (left {{eta}}) {{wide_msg}}"
    );
    bar.set_style(
        ProgressStyle::with_template(&template)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("=> "),
    );
    bar.set_prefix(verb.to_string());
    bar.enable_steady_tick(Duration::from_millis(200));
    bar
}

/// A mean colour difference, coloured: green when close to the game,
/// yellow when noticeable, red when something is clearly wrong.
pub fn mean(v: f32) -> StyledObject<String> {
    let text = format!("{v:>4.1}");
    if v < 3.0 {
        style(text).green()
    } else if v < 6.0 {
        style(text).yellow()
    } else {
        style(text).red().bold()
    }
}

/// A count that should be zero: green when it is, red otherwise.
pub fn count(n: usize) -> StyledObject<String> {
    if n == 0 {
        style(n.to_string()).green()
    } else {
        style(n.to_string()).red()
    }
}

/// Text that matters less, dimmed.
pub fn dim(text: impl Display) -> StyledObject<String> {
    style(text.to_string()).dim()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colours_follow_the_thresholds() {
        console::set_colors_enabled(false);
        assert_eq!(mean(2.4).to_string(), " 2.4");
        assert_eq!(count(0).to_string(), "0");
        assert_eq!(dim("x").to_string(), "x");
    }
}
