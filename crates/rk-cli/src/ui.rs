// Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
// SPDX-License-Identifier: LicenseRef-PolyForm-Shield-1.0.0

//! Terminal output in the style of cargo: a right-aligned coloured verb,
//! progress bars for long runs, and numbers coloured by how far they are
//! from the game. Colours and bars switch off on their own when the output
//! is not a terminal, so logs and pipes stay plain text.

use console::{style, StyledObject};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
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
    println!("{}", line_fail(&msg.to_string()));
}

/// A status line as text, for printing above a progress bar.
pub fn line(verb: &str, msg: &str) -> String {
    format!("{} {msg}", style(format!("{verb:>VERB$}")).green().bold())
}

/// A failure line as text, for printing above a progress bar.
pub fn line_fail(msg: &str) -> String {
    format!("{} {msg}", style(format!("{:>VERB$}", "error")).red().bold())
}

/// A section heading.
pub fn heading(text: impl Display) {
    println!("\n{}", style(text).bold().underlined());
}

/// A progress bar pinned to the bottom line, in apt's layout:
/// `Progress: [ 37%] [#####.....] 12/32  0:01:10 left 0:02:00  card`.
/// Lines printed through `bar.println` scroll above it.
pub fn bar(len: u64) -> ProgressBar {
    // On standard output, with the status lines, so the two never mix.
    let bar = ProgressBar::with_draw_target(Some(len), ProgressDrawTarget::stdout());
    let template = "{prefix:.black.on_green} [{percent:>3}%] [{bar:40.green}] {pos}/{len}  {elapsed_precise} left {eta_precise}  {wide_msg:.dim}";
    bar.set_style(
        ProgressStyle::with_template(template)
            .unwrap_or_else(|_| ProgressStyle::default_bar())
            .progress_chars("#."),
    );
    bar.set_prefix("Progress:");
    bar.enable_steady_tick(Duration::from_millis(200));
    bar
}

/// Prints a line above the bar, or plainly when the bar is hidden (output
/// is not a terminal), where indicatif would drop it.
pub fn above(bar: &ProgressBar, text: String) {
    if bar.is_hidden() {
        println!("{text}");
    } else {
        bar.println(text);
    }
}

/// How much a picture matches the game, in percent, from the mean colour
/// difference per pixel on the 0 to 255 scale: 2.55 is 1% off, a 99% match.
pub fn match_percent(mean: f32) -> f32 {
    100.0 - mean / 2.55
}

/// A match percentage from a mean colour difference, coloured: green when
/// close to the game (under 3 of 255 off), yellow when noticeable (under 6),
/// red when something is clearly wrong.
pub fn mean(v: f32) -> StyledObject<String> {
    let text = format!("{:>5.1}%", match_percent(v));
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
        assert_eq!(mean(2.55).to_string(), " 99.0%");
        assert_eq!(match_percent(0.0), 100.0);
        assert_eq!(count(0).to_string(), "0");
        assert_eq!(dim("x").to_string(), "x");
    }
}
