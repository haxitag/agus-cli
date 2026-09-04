use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::cli_format::OutputFormat;
use crate::cli_style;

pub struct CliSpinner {
    bar: Option<ProgressBar>,
}

impl CliSpinner {
    pub fn new(enabled: bool, message: &str) -> Self {
        if !enabled {
            eprintln!("{message}...");
            return Self { bar: None };
        }
        let bar = ProgressBar::new_spinner();
        bar.set_style(
            ProgressStyle::with_template("{spinner:.green} {msg}")
                .unwrap()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
        );
        bar.enable_steady_tick(Duration::from_millis(80));
        bar.set_message(message.to_string());
        Self { bar: Some(bar) }
    }

    pub fn set_message(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(message.to_string());
        }
    }

    pub fn finish_success(self, message: &str) {
        if let Some(bar) = self.bar {
            bar.finish_with_message(message.to_string());
        } else {
            println!("{message}");
        }
    }

    pub fn finish_and_clear(self) {
        if let Some(bar) = self.bar {
            bar.finish_and_clear();
        }
    }
}

pub fn use_progress(format: OutputFormat) -> bool {
    format.is_table() && !cli_style::is_piped_output()
}

pub fn use_progress_unconditional() -> bool {
    !cli_style::is_piped_output()
}
