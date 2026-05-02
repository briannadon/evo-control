//! evo-control — CLI and GUI for the Audient EVO 8.
//!
//! Usage:
//!   evo-control                        # launch GUI
//!   evo-control <subcommand>           # CLI mode
//!   evo-control --apply-default-preset # used by udev hook
//!
//! See `evo-control --help` for the full subcommand tree.

#![warn(clippy::all, unused)]

mod cli;
mod gui;
mod install;
mod probe;

use anyhow::Context;
use clap::Parser;

#[derive(Parser)]
#[command(
    name = "evo-control",
    version,
    about = "Control the Audient EVO 8 USB audio interface",
    long_about = "A Linux-native GUI + CLI for the Audient EVO 8 audio interface.\n\
                   Without subcommands, launches the graphical interface.\n\
                   With a subcommand, runs in CLI mode."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Apply the default preset on device connect (used by udev hook).
    #[arg(long, global = true)]
    apply_default_preset: bool,
}

#[derive(clap::Subcommand)]
enum Command {
    /// Set a control value.
    #[command(subcommand)]
    Set(SetCommand),

    /// Read a control value.
    Get {
        /// Control name: volume, gain, phantom, mute
        control: String,
        /// Target index or name (e.g. input number, "output")
        target: Option<String>,
    },

    /// Show device status.
    Status,

    /// Loopback mixer matrix operations.
    #[command(subcommand)]
    Mixer(MixerCommand),

    /// Preset management.
    #[command(subcommand)]
    Preset(PresetCommand),

    /// Walk all controls and verify protocol (requires hardware).
    Probe {
        /// Also test SET round-trip (writes original value back).
        #[arg(long)]
        probe_writable: bool,
    },

    /// Install kernel module and udev rules (requires sudo).
    InstallDriver,

    /// Uninstall kernel module and udev rules.
    UninstallDriver,
}

#[derive(clap::Subcommand)]
enum SetCommand {
    /// Set output volume in dB.
    Volume {
        /// Volume level in dB (-96..0)
        db: f32,
        /// Output pair index (0=main, 1=headphones).
        /// If omitted, applies to both pairs.
        #[arg(long)]
        pair: Option<u8>,
    },
    /// Set input gain in dB.
    Gain {
        /// Input number (1-4)
        input: u8,
        /// Gain level in dB (-8..50)
        db: f32,
    },
    /// Toggle phantom 48V power.
    Phantom {
        /// Input number (1-4)
        input: u8,
        /// on or off
        state: String,
    },
    /// Toggle mute.
    Mute {
        /// Target: input<N> or output
        target: String,
        /// on or off
        state: String,
    },
}

#[derive(clap::Subcommand)]
enum MixerCommand {
    /// Set a mixer cross-point level.
    Set {
        /// Input index (0-9)
        in_idx: u8,
        /// Output index (0-3)
        out_idx: u8,
        /// Level in dB (-128..6)
        #[arg(long)]
        db: f32,
    },
    /// Show current mixer matrix from the shadow store.
    Get,
    /// Reset mixer to hardware defaults.
    Reset,
}

#[derive(clap::Subcommand)]
enum PresetCommand {
    /// Save current state as a named preset.
    Save {
        /// Preset name
        name: String,
    },
    /// Load and apply a named preset.
    Load {
        /// Preset name
        name: String,
    },
    /// List all saved presets.
    List,
    /// Delete a named preset.
    Delete {
        /// Preset name
        name: String,
    },
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // --apply-default-preset flag: used by udev -> systemd hook
    if cli.apply_default_preset {
        return cli::apply_default_preset();
    }

    // Dispatch to GUI if no subcommand
    let cmd = match cli.command {
        Some(c) => c,
        None => return run_gui(),
    };

    // CLI mode: open the driver for commands that need it
    run_cli(cmd)
}

fn run_gui() -> anyhow::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_min_inner_size([600.0, 400.0])
            .with_title("evo-control"),
        ..Default::default()
    };
    eframe::run_native(
        "evo-control",
        options,
        Box::new(|_cc| Ok(Box::<gui::App>::default())),
    )
    .map_err(|e| anyhow::anyhow!("GUI error: {e}"))
}

fn run_cli(cmd: Command) -> anyhow::Result<()> {
    match cmd {
        // Commands that don't need the driver
        Command::Probe { probe_writable } => probe::probe(probe_writable),
        Command::InstallDriver => install::install_driver(),
        Command::UninstallDriver => install::uninstall_driver(),
        Command::Preset(PresetCommand::Save { name }) => cli::preset_save(&name),
        Command::Preset(PresetCommand::List) => cli::preset_list(),
        Command::Preset(PresetCommand::Delete { name }) => cli::preset_delete(&name),
        Command::Mixer(MixerCommand::Get) => cli::mixer_get(),
        Command::Mixer(MixerCommand::Reset) => {
            let (handle, _disc) = evo_driver::worker::spawn()?;
            cli::mixer_reset(&handle)
        },

        // Commands that need the driver
        cmd => {
            let (handle, _disc) = evo_driver::worker::spawn()
                .context("device not available — is it plugged in and the kmod loaded?\n\
                         Run `sudo evo-control install-driver` to install the kernel module.")?;

            match cmd {
                Command::Status => cli::status(&handle),
                Command::Get { control, target } => cli::get_control(&handle, &control, target.as_deref()),
                Command::Set(SetCommand::Volume { db, pair }) => cli::set_volume(&handle, db, pair),
                Command::Set(SetCommand::Gain { input, db }) => cli::set_gain(&handle, input, db),
                Command::Set(SetCommand::Phantom { input, state }) => {
                    let on = parse_on_off(&state)?;
                    cli::set_phantom(&handle, input, on)
                }
                Command::Set(SetCommand::Mute { target, state }) => {
                    let on = parse_on_off(&state)?;
                    cli::set_mute(&handle, &target, on)
                }
                Command::Mixer(MixerCommand::Set { in_idx, out_idx, db }) => {
                    cli::mixer_set(&handle, in_idx, out_idx, db)
                }
                Command::Mixer(MixerCommand::Reset) => cli::mixer_reset(&handle),
                Command::Preset(PresetCommand::Load { name }) => cli::preset_load(&handle, &name),
                // Unreachable — handled above
                Command::Probe { .. } | Command::InstallDriver | Command::UninstallDriver
                | Command::Preset(PresetCommand::Save { .. } | PresetCommand::List | PresetCommand::Delete { .. })
                | Command::Mixer(MixerCommand::Get) => unreachable!(),
            }
        }
    }
}

fn parse_on_off(s: &str) -> anyhow::Result<bool> {
    match s {
        "on" | "ON" | "1" | "true" => Ok(true),
        "off" | "OFF" | "0" | "false" => Ok(false),
        _ => anyhow::bail!("expected 'on' or 'off', got '{s}'"),
    }
}
