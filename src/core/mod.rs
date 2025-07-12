use colored::{Color, Colorize};
use ext_oscjson::AvatarIdentifier;
use glam::{Affine3A, Quat, Vec3};
use indicatif::MultiProgress;
use log::info;
use once_cell::sync::Lazy;
use rosc::{OscBundle, OscPacket, OscType};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
    time::{Duration, Instant},
};

use crate::Args;

use self::bundle::AvatarBundle;

// Module declarations for the different components of the application core.
mod bundle; // Handles OSC bundle creation.
mod ext_autopilot; // Manages autonomous avatar behaviors.
mod ext_gogo; // Implements "GoGo Loco" style movement adjustments.
mod ext_oscjson; // Handles OSC/JSON configuration for avatars.
mod ext_storage; // Manages persistent parameter storage.
mod ext_tracking; // Processes and forwards face and body tracking data.
mod folders; // Manages application-related folders.
mod watchdog; // A watchdog to ensure the application remains responsive.

// Public module for status bar management.
pub mod status;

// OSC address prefixes used for routing messages.
pub const PARAM_PREFIX: &str = "/avatar/parameters/";
const AVATAR_PREFIX: &str = "/avatar/change";
const TRACK_PREFIX: &str = "/tracking/trackers/";
const INPUT_PREFIX: &str = "/input/";

/// A type alias for a HashMap storing avatar parameters, mapping parameter names to OSC types.
pub type AvatarParameters = HashMap<Arc<str>, OscType>;

/// Represents the shared state of the application.
/// This struct is passed to various components to allow them to access and modify
/// tracking data, parameters, and other global state.
pub struct AppState {
    /// OSC tracking data for head and hands.
    pub tracking: OscTrack,
    /// Current avatar parameters.
    pub params: AvatarParameters,
    /// The status bar manager for displaying UI in the terminal.
    pub status: status::StatusBar,
    /// A flag to control the application's main loop, indicating whether it should self-drive or wait for VSync.
    pub self_drive: Arc<AtomicBool>,
    /// The time elapsed since the last frame, in seconds.
    pub delta_t: f32,
}

/// The main struct for the Avatar OSC application.
/// It manages OSC communication, extensions, and the main application loop.
pub struct AvatarOsc {
    osc_port: u16,
    upstream: UdpSocket,
    ext_autopilot: ext_autopilot::ExtAutoPilot,
    ext_oscjson: ext_oscjson::ExtOscJson,
    ext_storage: ext_storage::ExtStorage,
    ext_gogo: ext_gogo::ExtGogo,
    ext_tracking: ext_tracking::ExtTracking,
    multi: MultiProgress,
    avatar_file: Option<String>,
}

/// Holds OSC tracking data for the head and hands.
pub struct OscTrack {
    pub head: Affine3A,
    pub left_hand: Affine3A,
    pub right_hand: Affine3A,
    /// The timestamp of the last received tracking data.
    pub last_received: Instant,
}

impl AvatarOsc {
    /// Creates a new `AvatarOsc` instance.
    ///
    /// # Arguments
    ///
    /// * `args` - Command line arguments.
    /// * `multi` - A `MultiProgress` instance for managing terminal progress bars.
    pub fn new(args: Args, multi: MultiProgress) -> AvatarOsc {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);

        // Set up the UDP socket to send OSC messages to the game (e.g., VRChat).
        let upstream = UdpSocket::bind("0.0.0.0:0").expect("bind upstream socket");
        upstream
            .connect(SocketAddr::new(ip, args.vrc_port))
            .expect("upstream connect");

        // Initialize all the extensions.
        let ext_autopilot = ext_autopilot::ExtAutoPilot::new();
        let ext_storage = ext_storage::ExtStorage::new();
        let ext_gogo = ext_gogo::ExtGogo::new();
        let ext_tracking = ext_tracking::ExtTracking::new(args.face);
        let ext_oscjson = ext_oscjson::ExtOscJson::new();

        AvatarOsc {
            osc_port: args.osc_port,
            upstream,
            ext_autopilot,
            ext_oscjson,
            ext_storage,
            ext_gogo,
            ext_tracking,
            multi,
            avatar_file: args.avatar,
        }
    }

    /// Sends a buffer of data to the upstream OSC endpoint (the game).
    pub fn send_upstream(&self, buf: &[u8]) -> std::io::Result<usize> {
        self.upstream.send(buf)
    }

    /// The main message handling loop of the application.
    /// It listens for incoming OSC messages, processes them, and drives the application state.
    pub fn handle_messages(&mut self) {
        let ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
        let listener =
            UdpSocket::bind(SocketAddr::new(ip, self.osc_port)).expect("bind listener socket");

        // A loopback socket to self-trigger the processing loop when in self-driven mode.
        let lo = UdpSocket::bind("0.0.0.0:0").expect("bind self socket");
        lo.connect(SocketAddr::new(ip, self.osc_port)).unwrap();
        let lo_addr = lo.local_addr().unwrap();

        // Initialize the application state.
        let mut state = AppState {
            status: status::StatusBar::new(&self.multi),
            params: AvatarParameters::new(),
            tracking: OscTrack {
                head: Affine3A::IDENTITY,
                left_hand: Affine3A::IDENTITY,
                right_hand: Affine3A::IDENTITY,
                last_received: Instant::now(),
            },
            self_drive: Arc::new(AtomicBool::new(true)),
            delta_t: 0.011f32,
        };

        // Start the watchdog to monitor responsiveness.
        let watchdog = watchdog::Watchdog::new(state.self_drive.clone());
        watchdog.run();
        // Spawn a thread to periodically send a message to the loopback socket if in self-drive mode.
        // This ensures the `process` function is called regularly.
        thread::spawn({
            let drive = state.self_drive.clone();
            move || loop {
                if drive.load(Ordering::Relaxed) {
                    let _ = lo.send(&[0u8; 1]);
                    thread::sleep(Duration::from_millis(11)); // ~90 Hz
                } else {
                    // If not in self-drive mode, sleep longer as we wait for VSync messages.
                    thread::sleep(Duration::from_millis(200));
                }
            }
        });

        info!(
            "Listening for OSC messages on {}",
            listener.local_addr().unwrap()
        );

        let mut last_frame = Instant::now();
        let mut buf = [0u8; rosc::decoder::MTU];
        loop {
            if let Ok((size, addr)) = listener.recv_from(&mut buf) {
                // If the message is from our loopback socket, it's a tick for the process loop.
                if addr == lo_addr {
                    self.process(&mut state);
                    watchdog.update();
                    state.delta_t = last_frame.elapsed().as_secs_f32();
                    last_frame = Instant::now();
                    continue;
                }

                // Decode the received UDP packet as an OSC message.
                if let Ok((_, OscPacket::Message(packet))) = rosc::decoder::decode_udp(&buf[..size])
                {
                    state.status.trip_recv_counter();
                    // Handle avatar parameter changes.
                    if packet.addr.starts_with(PARAM_PREFIX) {
                        let name: Arc<str> = packet.addr[PARAM_PREFIX.len()..].into();
                        // The "VSync" parameter is special: it drives the main loop when available.
                        if &*name == "VSync" {
                            state.self_drive.store(false, Ordering::Relaxed);
                            self.process(&mut state);
                            state.delta_t = last_frame.elapsed().as_secs_f32();
                            last_frame = Instant::now();
                            watchdog.update();
                        } else if let Some(arg) = packet.args.into_iter().next() {
                            // Notify extensions of parameter changes and update the state.
                            self.ext_storage.notify(&name, &arg);
                            self.ext_gogo.notify(&name, &arg);
                            state.params.insert(name, arg);
                        }
                    // Handle tracker data.
                    } else if packet.addr.starts_with(TRACK_PREFIX) {
                        if let [OscType::Float(x), OscType::Float(y), OscType::Float(z), OscType::Float(ex), OscType::Float(ey), OscType::Float(ez)] =
                            packet.args[..]
                        {
                            let transform = Affine3A::from_rotation_translation(
                                Quat::from_euler(glam::EulerRot::ZXY, ex, ey, ez),
                                Vec3::new(x, y, z),
                            );

                            if packet.addr[TRACK_PREFIX.len()..].starts_with("head") {
                                state.tracking.last_received = Instant::now();
                                state.tracking.head = transform;
                            } else if packet.addr[TRACK_PREFIX.len()..].starts_with("leftwrist") {
                                state.tracking.left_hand = transform;
                            } else if packet.addr[TRACK_PREFIX.len()..].starts_with("rightwrist") {
                                state.tracking.right_hand = transform;
                            }
                        }
                    // Handle avatar changes.
                    } else if packet.addr.starts_with(AVATAR_PREFIX) {
                        if let [OscType::String(avatar)] = &packet.args[..] {
                            self.avatar(AvatarIdentifier::Uid(avatar.clone()), &mut state);
                        }
                    } else {
                        log::info!("Received data: {:?}", packet);
                    }
                }
            };
        }
    }

    /// Handles avatar changes. This is called when a `/avatar/change` message is received.
    /// It loads the new avatar's OSC JSON configuration and notifies extensions.
    fn avatar(&mut self, avatar: AvatarIdentifier, state: &mut AppState) {
        info!("Avatar changed: {:?}", avatar);
        let osc_root_node = self.ext_oscjson.avatar(&avatar);
        if let Some(osc_root_node) = osc_root_node.as_ref() {
            self.ext_tracking.osc_json(osc_root_node);
        }

        // Let the GoGo extension know about the avatar change.
        let mut bundle = OscBundle::new_bundle();
        self.ext_gogo.avatar(&mut bundle);
        bundle
            .serialize()
            .and_then(|buf| self.send_upstream(&buf).ok());

        // Determine if the application should be self-driven or VSync-driven based on the new avatar's capabilities.
        state.self_drive.store(
            !osc_root_node.is_some_and(|n| {
                let has_vsync = n.has_vsync();

                let vsync_name = "VSync".color(Color::BrightYellow);

                if !has_vsync {
                    log::warn!(
                        "This avatar does not have a {} parameter, falling back to {} mode.",
                        vsync_name,
                        *DRIVE_ON,
                    );
                    log::warn!(
                        "The {} parameter helps OscAvMgr keep in sync with your avatar's animator.",
                        vsync_name
                    );
                    log::warn!(
                        "Consider implementing a {} parameter using either:",
                        vsync_name
                    );
                    log::warn!("- a bool param that flips every animator frame.");
                    log::warn!("- a float param that randomizes each animator frame.");
                }
                has_vsync
            }),
            Ordering::Relaxed,
        );
    }

    /// Processes a single frame of the application logic.
    /// This function is called on every "tick", either self-driven or by a VSync message.
    fn process(&mut self, state: &mut AppState) {
        let mut bundle = OscBundle::new_bundle();

        // Update status bar items.
        state
            .status
            .add_item(match state.self_drive.load(Ordering::Relaxed) {
                true => DRIVE_ON.clone(),
                false => DRIVE_OFF.clone(),
            });

        state.status.add_item(
            match state.tracking.last_received.elapsed() < Duration::from_secs(1) {
                true => TRACK_ON.clone(),
                false => TRACK_OFF.clone(),
            },
        );

        // Check for avatar changes from OSC JSON or command line arguments.
        if self.ext_oscjson.step() {
            self.avatar(AvatarIdentifier::Default, state);
        } else if let Some(path) = self.avatar_file.take() {
            self.avatar(AvatarIdentifier::Path(path.clone()), state);
        }

        // Step through each extension, allowing them to add messages to the OSC bundle.
        self.ext_storage.step(&mut bundle);
        self.ext_tracking.step(state, &mut bundle);
        self.ext_gogo.step(&state.params, &mut bundle);
        self.ext_autopilot
            .step(state, &self.ext_tracking, &mut bundle);

        // If the first item in the bundle is a single message, send it immediately.
        // This is likely for low-latency updates.
        if let Some(packet) = bundle.content.first() {
            if let OscPacket::Message(..) = packet {
                rosc::encoder::encode(packet)
                    .ok()
                    .and_then(|buf| self.send_upstream(&buf).ok());
                bundle.content.remove(0);
            }
        }

        // Update and display status counters.
        state.status.trip_fps_counter();
        state.status.set_sent_count(bundle.content.len() as _);
        state.status.recv_summary();

        // Chunk the remaining bundle content and send it upstream.
        // This avoids sending UDP packets that are too large.
        for bundle in bundle.content.chunks(30).map(|chunk| {
            let mut bundle = OscBundle::new_bundle();
            bundle.content.extend_from_slice(chunk);
            bundle
        }) {
            bundle
                .serialize()
                .and_then(|buf| self.send_upstream(&buf).ok());
        }

        state.status.display();
    }
}

// Static lazy-initialized strings for colored status indicators in the terminal.
static DRIVE_ON: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "DRIVE".color(Color::Blue)).into());
static DRIVE_OFF: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "VSYNC".color(Color::Green)).into());

pub static TRACK_ON: Lazy<Arc<str>> =
    Lazy::new(|| format!("{}", "TRACK".color(Color::Green)).into());
pub static TRACK_OFF: Lazy<Arc<str>> =
    Lazy::new(|| format!("{}", "TRACK".color(Color::Red)).into());

// Static lazy-initialized strings for instruction headers in the terminal.
pub static INSTRUCTIONS_START: Lazy<Arc<str>> = Lazy::new(|| {
    format!(
        "{}{}{}",
        "==".color(Color::BrightBlue),
        "Instructions".color(Color::BrightYellow),
        "================================".color(Color::BrightBlue)
    )
    .into()
});

pub static INSTRUCTIONS_END: Lazy<Arc<str>> = Lazy::new(|| {
    format!(
        "{}{}{}",
        "================================".color(Color::BrightBlue),
        "Instructions".color(Color::BrightYellow),
        "==".color(Color::BrightBlue)
    )
    .into()
});
