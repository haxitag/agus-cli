use std::io::{self, IsTerminal};
use std::sync::atomic::{AtomicBool, Ordering};

use agus_core_domain::{Environment, Host};
use comfy_table::{presets::UTF8_FULL, Attribute, Cell, Color, Table};
use owo_colors::OwoColorize;

static COLOR_ENABLED: AtomicBool = AtomicBool::new(true);

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

impl ColorMode {
    pub fn init(self) {
        let enabled = match self {
            ColorMode::Always => true,
            ColorMode::Never => false,
            ColorMode::Auto => {
                if std::env::var("NO_COLOR").is_ok() {
                    false
                } else {
                    io::stdout().is_terminal()
                }
            }
        };
        COLOR_ENABLED.store(enabled, Ordering::Relaxed);
    }
}

fn use_color() -> bool {
    COLOR_ENABLED.load(Ordering::Relaxed)
}

pub fn print_ok(message: &str) {
    if use_color() {
        println!("{} {}", "✓".green().bold(), message);
    } else {
        println!("OK\t{message}");
    }
}

pub fn print_fail(message: &str) {
    if use_color() {
        eprintln!("{} {}", "✗".red().bold(), message);
    } else {
        eprintln!("FAIL\t{message}");
    }
}

pub fn print_warn(message: &str) {
    if use_color() {
        eprintln!("{} {}", "!".yellow().bold(), message);
    } else {
        eprintln!("WARN\t{message}");
    }
}

pub fn print_host_check_ok(host_id: &str, address: &str) {
    if use_color() {
        println!(
            "{} {} {}",
            "OK".green().bold(),
            host_id.bold(),
            address.dimmed()
        );
    } else {
        println!("OK\t{host_id}\t{address}");
    }
}

pub fn print_host_check_fail(host_id: &str, address: &str, detail: &str) {
    if use_color() {
        eprintln!(
            "{} {} {} {}",
            "FAIL".red().bold(),
            host_id.bold(),
            address.dimmed(),
            detail.red()
        );
    } else {
        eprintln!("FAIL\t{host_id}\t{address}\t{detail}");
    }
}

pub fn print_host_table(hosts: &[Host]) {
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    if use_color() {
        table.set_header(vec![
            Cell::new("ID").add_attribute(Attribute::Bold),
            Cell::new("ADDRESS").add_attribute(Attribute::Bold),
            Cell::new("USER").add_attribute(Attribute::Bold),
            Cell::new("PORT").add_attribute(Attribute::Bold),
            Cell::new("ENV").add_attribute(Attribute::Bold),
            Cell::new("LABELS").add_attribute(Attribute::Bold),
        ]);
    } else {
        table.set_header(vec!["ID", "ADDRESS", "USER", "PORT", "ENV", "LABELS"]);
    }

    for host in hosts {
        table.add_row(vec![
            Cell::new(&host.id),
            Cell::new(&host.address),
            Cell::new(&host.user),
            Cell::new(host.port.to_string()),
            env_cell(&host.environment),
            Cell::new(host.labels.join(",")),
        ]);
    }

    println!("{table}");
}

pub fn print_host_show(host: &Host) {
    if use_color() {
        println!("{} {}", "id".bold(), host.id);
        println!("{} {}", "address".bold(), host.address);
        println!("{} {}", "user".bold(), host.user);
        println!("{} {}", "port".bold(), host.port);
        println!("{} {:?}", "environment".bold(), host.environment);
        println!("{} {}", "labels".bold(), host.labels.join(","));
        if let Some(group) = &host.group_id {
            println!("{} {group}", "group".bold());
        }
        if let Some(identity) = &host.identity_file {
            println!("{} {identity}", "identity_file".bold());
        }
    } else {
        println!("id: {}", host.id);
        println!("address: {}", host.address);
        println!("user: {}", host.user);
        println!("port: {}", host.port);
        println!("environment: {:?}", host.environment);
        println!("labels: {}", host.labels.join(","));
        if let Some(group) = &host.group_id {
            println!("group: {group}");
        }
        if let Some(identity) = &host.identity_file {
            println!("identity_file: {identity}");
        }
    }
}

fn env_cell(env: &Environment) -> Cell {
    let label = format!("{env:?}");
    if !use_color() {
        return Cell::new(label);
    }
    match env {
        Environment::Prod => Cell::new(label).fg(Color::Red),
        Environment::Staging => Cell::new(label).fg(Color::Yellow),
        Environment::Dev => Cell::new(label).fg(Color::Green),
        Environment::Test => Cell::new(label).fg(Color::Magenta),
    }
}

pub fn is_piped_output() -> bool {
    !io::stdout().is_terminal()
}
