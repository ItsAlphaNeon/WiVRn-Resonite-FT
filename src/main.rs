#![allow(dead_code)]

use crate::core::AvatarOsc;

use clap::Parser;
use env_logger::Env;
use indicatif::MultiProgress;
use indicatif_log_bridge::LogWrapper;

mod core;

/// The main entry point of the application.
fn main() {
    // Initialize the logger using `env_logger`.
    // This allows configuring log levels via the `RUST_LOG` environment variable.
    // It's configured to filter out noisy messages from `mdns_sd` and format logs concisely.
    let log = env_logger::Builder::from_env(Env::default().default_filter_or("info"))
        .filter_module("mdns_sd", log::LevelFilter::Warn)
        .format_target(false)
        .format_module_path(false)
        .build();
    // `MultiProgress` is used to manage multiple progress bars in the terminal.
    let multi = MultiProgress::new();
    // `LogWrapper` bridges the `log` crate with `indicatif`'s progress bars,
    // ensuring that log messages don't mess up the progress bar display.
    LogWrapper::new(multi.clone(), log).try_init().unwrap();

    // Parse command-line arguments using `clap`.
    let args = Args::parse();

    // Create a new instance of the main application struct, `AvatarOsc`.
    let mut osc = AvatarOsc::new(args, multi);

    // Start the main message handling loop. This function will run indefinitely.
    osc.handle_messages();
}

/// Defines the available face tracking setups as subcommands for the command-line interface.
/// This enum is used by `clap` to parse which face tracking provider the user wants to use.
#[derive(Default, Debug, Clone, clap::Subcommand)]
pub enum FaceSetup {
    #[default]
    #[clap(subcommand, hide = true)]
    /// Do not use face tracking. This is the default option.
    Dummy,
    #[cfg(feature = "openxr")]
    /// Retrieve face data from OpenXR (e.g., WiVRn / Monado).
    /// This option is only available if the "openxr" feature is enabled during compilation.
    Openxr,

    #[cfg(feature = "alvr")]
    /// Retrieve face data from ALVR.
    /// This option is only available if the "alvr" feature is enabled during compilation.
    Alvr,

    #[cfg(feature = "babble")]
    /// Retrieve face data from Babble and Etvr.
    /// This option is only available if the "babble" feature is enabled during compilation.
    Babble {
        /// The port to listen on for Babble and ETVR packets.
        #[arg(short, long, default_value = "9400")]
        listen: u16,
    },
}

/// Defines the command-line arguments for the OSC Avatar Manager application.
/// `clap::Parser` automatically generates a command-line parser from this struct.
#[derive(Default, clap::Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Args {
    /// Provider to use for face data. This is a subcommand that uses the `FaceSetup` enum.
    #[command(subcommand)]
    face: FaceSetup,

    /// The OSC port that VRChat (or a similar application) is listening on.
    #[arg(long, default_value = "9000")]
    vrc_port: u16,

    /// The port this application will listen on for incoming OSC messages from VRChat.
    #[arg(long, default_value = "9002")]
    osc_port: u16,

    /// An optional path to an OSC-JSON avatar configuration file.
    /// If not provided, a default path will be used.
    #[arg(long)]
    avatar: Option<String>,
}
