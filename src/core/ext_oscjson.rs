use log::{info, warn};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use rosc::{OscBundle, OscType};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Write},
    sync::Arc,
    thread,
    time::Duration,
};

use super::{bundle::AvatarBundle, folders::CONFIG_DIR};

/// This extension handles the discovery and interaction with an OSC JSON service,
/// typically provided by a VR application like VRChat or Resonite. It allows the application
/// to dynamically learn the OSC address space of the current avatar, including all available parameters.
pub struct ExtOscJson {
    /// The mDNS (Bonjour/Zeroconf) service daemon used to discover OSC JSON services on the local network.
    mdns: ServiceDaemon,
    /// The receiver channel for mDNS service events.
    mdns_recv: mdns_sd::Receiver<ServiceEvent>,
    /// The discovered network address (e.g., "http://127.0.0.1:9001/avatar") of the OSC JSON service.
    oscjson_addr: Option<Arc<str>>,
    /// A timestamp to throttle how frequently the service discovery is performed.
    next_run: std::time::Instant,
    /// An HTTP client for making requests to the OSC JSON service.
    client: reqwest::blocking::Client,
}

impl ExtOscJson {
    /// Initializes the OSC JSON extension.
    pub fn new() -> Self {
        // Create a new mDNS daemon to listen for network services.
        let mdns = ServiceDaemon::new().unwrap();
        // Start browsing for services of the type "_oscjson._tcp.local.", which is the standard for OSC JSON.
        let mdns_recv = mdns.browse("_oscjson._tcp.local.").unwrap();
        let client = reqwest::blocking::Client::new();

        Self {
            mdns,
            mdns_recv,
            oscjson_addr: None,
            next_run: std::time::Instant::now(),
            client,
        }
    }

    /// The main update loop for the extension, called periodically.
    /// It checks for new OSC JSON services on the network.
    /// Returns `true` if a new avatar service was discovered in this step.
    pub fn step(&mut self) -> bool {
        let mut notify_avatar = false;
        // Throttle the check to avoid excessive network activity.
        if self.next_run > std::time::Instant::now() {
            return notify_avatar;
        }
        self.next_run = std::time::Instant::now() + std::time::Duration::from_secs(15);

        // Process all pending mDNS events.
        for event in self.mdns_recv.try_iter() {
            if let ServiceEvent::ServiceResolved(info) = event {
                // We only care about services published by the VRChat client.
                if !info.get_fullname().starts_with("VRChat-Client-") {
                    continue;
                }
                let addr = info.get_addresses().iter().next().unwrap();
                info!(
                    "Found OSCJSON service: {} @ {}:{}",
                    info.get_fullname(),
                    addr,
                    info.get_port()
                );

                // If this is the first time we're discovering the address, flag it.
                if self.oscjson_addr.is_none() {
                    notify_avatar = true;
                }

                // Store the constructed URL to the avatar's OSC JSON definition.
                self.oscjson_addr =
                    Some(format!("http://{}:{}/avatar", addr, info.get_port()).into());
            }
        }

        // If a new avatar was found, immediately fetch its JSON definition.
        if self.oscjson_addr.is_some() && notify_avatar {
            self.avatar(&AvatarIdentifier::Default);
        }
        notify_avatar
    }

    /// Fetches, parses, and saves the avatar's OSC JSON definition.
    ///
    /// # Arguments
    /// * `avatar` - An `AvatarIdentifier` specifying whether to fetch from the network (`Default`) or a local file (`Path`).
    ///
    /// # Returns
    /// An `Option<OscJsonNode>` containing the root of the parsed avatar parameter tree.
    pub fn avatar(&mut self, avatar: &AvatarIdentifier) -> Option<OscJsonNode> {
        let mut json = String::new();

        if let AvatarIdentifier::Path(path) = avatar {
            // Load from a local file if a path is provided.
            if let Err(e) = File::open(path).and_then(|mut f| f.read_to_string(&mut json)) {
                log::error!("Could not read file: {:?}", e);
                return None;
            }
        } else {
            // Otherwise, fetch from the discovered network service.
            let Some(addr) = self.oscjson_addr.as_ref() else {
                warn!("No avatar oscjson address.");
                return None;
            };

            // A small delay, possibly to ensure the service is fully ready to respond.
            thread::sleep(Duration::from_millis(250));

            let Ok(resp) = self.client.get(addr.as_ref()).send() else {
                warn!("Failed to send avatar json request.");
                return None;
            };

            let Ok(text) = resp.text() else {
                warn!("No payload in avatar json response.");
                return None;
            };

            json = text;
        }

        // Save a local copy of the fetched JSON for debugging or later use.
        let path = format!("{}/{}", CONFIG_DIR.as_ref(), "oscavmgr-avatar.json");
        if let Err(e) = File::create(path).and_then(|mut f| f.write_all(json.as_bytes())) {
            warn!("Could not write avatar json file: {:?}", e);
        }

        // Parse the JSON string into the OscJsonNode structure.
        match serde_json::from_str(&json) {
            Ok(root_node) => Some(root_node),
            Err(e) => {
                warn!("Failed to deserialize avatar json: {}", e);
                None
            }
        }
    }
}

/// An enum to identify the source of an avatar's OSC JSON definition.
#[derive(Debug)]
pub enum AvatarIdentifier {
    /// Use the default, network-discovered service.
    Default,
    /// Identify by a unique ID (not currently used).
    Uid(String),
    /// Load from a local file path.
    Path(String),
}

/// Represents a node in the OSC JSON hierarchy, which describes an avatar's OSC parameters.
#[derive(Serialize, Deserialize, Debug)]
pub struct OscJsonNode {
    /// The full OSC address path for this node (e.g., "/avatar/parameters/JawOpen").
    #[serde(alias = "FULL_PATH")]
    pub full_path: Arc<str>,
    /// An integer indicating access rights (e.g., 1 for read, 2 for write, 3 for read/write).
    #[serde(alias = "ACCESS")]
    pub access: i32,
    /// The expected OSC data type for this parameter (e.g., "Float", "Int", "Bool").
    #[serde(alias = "TYPE")]
    pub data_type: Option<Arc<str>>,
    /// A map of child nodes, representing the nested structure of the OSC address space.
    #[serde(alias = "CONTENTS")]
    pub contents: Option<HashMap<Arc<str>, OscJsonNode>>,
}

impl OscJsonNode {
    /// Traverses the node tree to find a specific node by its relative path.
    pub fn get(&self, path: &str) -> Option<&OscJsonNode> {
        let mut node = self;
        for part in path.split('/') {
            if let Some(contents) = &node.contents {
                node = contents.get(part)?;
            } else {
                return None;
            }
        }
        Some(node)
    }

    /// A specific helper to check if the avatar supports the "VSync" parameter,
    /// which can be used for timing adjustments.
    pub fn has_vsync(&self) -> bool {
        self.get("parameters")
            .and_then(|parameters| parameters.get("VSync"))
            .is_some()
    }
}

/// This struct represents a complex avatar parameter that is controlled by multiple OSC addresses.
/// This is common for parameters that are "bit-packed" into several boolean values for higher precision
/// over the standard 8-bit float range of OSC.
#[derive(Clone)]
pub struct MysteryParam {
    pub name: Arc<str>,
    /// The primary address, which usually takes a float value.
    pub main_address: Option<Arc<str>>,
    /// An array of addresses for the individual bits of a high-precision value.
    pub addresses: [Option<Arc<str>>; 7],
    /// An address for a boolean that represents the sign of the value.
    pub neg_address: Option<Arc<str>>,
    /// The number of bits used for the high-precision value.
    pub num_bits: usize,
    /// The last float value sent to the main address, for change detection.
    pub last_value: f32,
    /// The last state of the boolean bits sent, for change detection.
    pub last_bits: [bool; 8],
}

impl MysteryParam {
    /// Sends the given float value to the appropriate OSC addresses for this parameter.
    /// It handles sending to the main float address as well as updating the individual boolean bits.
    pub fn send(&mut self, value: f32, bundle: &mut OscBundle) {
        // Send to the main float address if it exists and the value has changed.
        if let Some(addr) = self.main_address.as_ref() {
            if (value - self.last_value).abs() > 0.01 {
                bundle.send_parameter(addr, OscType::Float(value));
                self.last_value = value;
            }
        }

        let mut value = value;
        // Handle the negative sign bit if it exists.
        if let Some(addr) = self.neg_address.as_ref() {
            let send_val = value < 0.;
            if self.last_bits[7] != send_val {
                bundle.send_parameter(addr, OscType::Bool(send_val));
                self.last_bits[7] = send_val;
            }
            value = value.abs();
        } else if value < 0. {
            value = 0.; // If there's no negative address, clamp to positive.
        }

        // Convert the float value (0.0-1.0) to an integer based on the number of bits.
        let value = (value * ((1 << self.num_bits) - 1) as f32) as i32;

        // Iterate through the bits and send boolean updates if they have changed.
        self.addresses
            .iter()
            .enumerate()
            .take(self.num_bits)
            .for_each(|(idx, param)| {
                if let Some(addr) = param.as_ref() {
                    let send_val = value & (1 << idx) != 0;
                    if self.last_bits[idx] != send_val {
                        bundle.send_parameter(addr, OscType::Bool(send_val));
                        self.last_bits[idx] = send_val;
                    }
                }
            });
    }
}
