//! This module defines the `AvatarBundle` trait and its implementation for `rosc::OscBundle`.
//! It provides a structured way to create and manage OSC (Open Sound Control) bundles
//! for controlling avatars in applications like VRChat or Resonite.

use rosc::{OscBundle, OscMessage, OscPacket, OscType};

use super::{INPUT_PREFIX, PARAM_PREFIX};

/// A trait for building OSC (Open Sound Control) bundles to send to applications like Resonite.
///
/// This trait abstracts the creation of OSC messages for various avatar interactions,
/// such as controlling parameters (like blendshapes), sending tracking data, and simulating user input.
/// By using a trait, the application can easily switch between different OSC library implementations
/// or mock the bundle creation for testing purposes.
pub trait AvatarBundle {
    /// Creates a new, empty bundle.
    /// This serves as the starting point for assembling a collection of OSC messages.
    fn new_bundle() -> Self;

    /// Adds a message to the bundle to set an avatar parameter.
    ///
    /// Parameters are typically used to control blendshapes for facial expressions,
    /// but can be used for any animatable property on the avatar.
    ///
    /// # Arguments
    /// * `name` - The name of the parameter to set (e.g., "JawOpen").
    /// * `value` - The value of the parameter, encapsulated in an `OscType`.
    fn send_parameter(&mut self, name: &str, value: OscType);

    /// Adds a raw tracking data message to the bundle.
    ///
    /// This is used for sending high-frequency data like head or eye tracking coordinates.
    ///
    /// # Arguments
    /// * `addr` - The OSC address for the tracking data (e.g., "/tracking/eye/left").
    /// * `args` - A vector of `OscType` values representing the tracking data.
    fn send_tracking(&mut self, addr: &str, args: Vec<OscType>);

    /// Adds a message to simulate an analog input axis.
    ///
    /// This can be used to control avatar movement or other continuous actions.
    ///
    /// # Arguments
    /// * `name` - The name of the input axis (e.g., "Vertical").
    /// * `value` - The value of the axis, typically from -1.0 to 1.0.
    fn send_input_axis(&mut self, name: &str, value: f32);

    /// Adds a message to simulate a button press or release.
    ///
    /// # Arguments
    /// * `name` - The name of the input button (e.g., "Jump").
    /// * `value` - `true` for pressed, `false` for released.
    fn send_input_button(&mut self, name: &str, value: bool);

    /// Adds a message to be displayed in the in-game chatbox.
    ///
    /// # Arguments
    /// * `message` - The text to display.
    /// * `open_keyboard` - If `true`, suggests the game open the virtual keyboard.
    /// * `play_sound` - If `true`, a notification sound is played.
    fn send_chatbox_message(&mut self, message: String, open_keyboard: bool, play_sound: bool);

    /// Serializes the entire bundle into a byte vector for transmission over the network.
    ///
    /// If the bundle contains no messages, this returns `None` to avoid sending empty packets.
    fn serialize(self) -> Option<Vec<u8>>;
}

/// Implements the `AvatarBundle` trait for the `rosc::OscBundle` struct.
/// This provides a concrete implementation for creating and serializing OSC bundles
/// using the `rosc` crate.
impl AvatarBundle for OscBundle {
    /// Creates a new `OscBundle` with a default (immediate) timestamp and an empty content vector.
    fn new_bundle() -> OscBundle {
        OscBundle {
            timetag: rosc::OscTime {
                seconds: 0,
                fractional: 0,
            },
            content: Vec::new(),
        }
    }

    /// Adds an OSC message to the bundle for an avatar parameter.
    /// The OSC address is constructed by prepending the `PARAM_PREFIX` (e.g., "/avatar/parameters/").
    fn send_parameter(&mut self, name: &str, value: OscType) {
        log::trace!("Sending parameter {} = {:?}", name, value);
        self.content.push(OscPacket::Message(OscMessage {
            addr: format!("{}{}", PARAM_PREFIX, name),
            args: vec![value],
        }));
    }

    /// Adds a raw OSC tracking message to the bundle.
    fn send_tracking(&mut self, addr: &str, args: Vec<OscType>) {
        log::trace!("Sending tracking {} = {:?}", addr, args);
        self.content.push(OscPacket::Message(OscMessage {
            addr: addr.to_string(),
            args,
        }));
    }

    /// Adds an OSC message for an input axis.
    /// The OSC address is constructed by prepending the `INPUT_PREFIX` (e.g., "/input/").
    fn send_input_axis(&mut self, name: &str, value: f32) {
        log::trace!("Sending input axis {} = {:?}", name, value);
        self.content.push(OscPacket::Message(OscMessage {
            addr: format!("{}{}", INPUT_PREFIX, name),
            args: vec![OscType::Float(value)],
        }));
    }

    /// Adds an OSC message for an input button.
    /// The OSC address is constructed by prepending the `INPUT_PREFIX` (e.g., "/input/").
    fn send_input_button(&mut self, name: &str, value: bool) {
        log::trace!("Sending input button {} = {:?}", name, value);
        self.content.push(OscPacket::Message(OscMessage {
            addr: format!("{}{}", INPUT_PREFIX, name),
            args: vec![OscType::Bool(value)],
        }));
    }

    /// Inserts a chatbox message at the beginning of the bundle's message list.
    /// This can give it priority in processing, though OSC message order is not guaranteed.
    fn send_chatbox_message(&mut self, message: String, open_keyboard: bool, play_sound: bool) {
        log::trace!(
            "Sending chatbox message {} (kbd: {:?}, sfx: {:?})",
            message,
            open_keyboard,
            play_sound
        );
        self.content.insert(
            0,
            OscPacket::Message(OscMessage {
                addr: "/chatbox/input/".to_string(),
                args: vec![
                    OscType::String(message),
                    OscType::Bool(open_keyboard),
                    OscType::Bool(play_sound),
                ],
            }),
        );
    }

    /// Encodes the `OscBundle` into a `Vec<u8>`.
    /// Returns `None` if the bundle is empty to prevent sending unnecessary network traffic.
    fn serialize(self) -> Option<Vec<u8>> {
        if !self.content.is_empty() {
            rosc::encoder::encode(&OscPacket::Bundle(self)).ok()
        } else {
            None
        }
    }
}
