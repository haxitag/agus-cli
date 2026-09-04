use agus_cli_core::CliError;
use clap::ValueEnum;
use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
}

impl OutputFormat {
    pub fn is_structured(self) -> bool {
        matches!(self, Self::Json | Self::Yaml)
    }

    pub fn is_table(self) -> bool {
        matches!(self, Self::Table)
    }
}

pub fn print_value<T: Serialize>(format: OutputFormat, value: &T) -> Result<(), CliError> {
    match format {
        OutputFormat::Json => {
            let text = serde_json::to_string_pretty(value)
                .map_err(|err| CliError::InvalidInput(err.to_string()))?;
            println!("{text}");
        }
        OutputFormat::Yaml => {
            let text = serde_yaml::to_string(value)
                .map_err(|err| CliError::InvalidInput(err.to_string()))?;
            print!("{text}");
            if !text.ends_with('\n') {
                println!();
            }
        }
        OutputFormat::Table => {}
    }
    Ok(())
}
