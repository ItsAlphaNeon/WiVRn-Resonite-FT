use std::{array, str::FromStr, sync::Arc};

use once_cell::sync::Lazy;
use regex::Regex;
use rosc::{OscBundle, OscType};
use sranipal::SRanipalExpression;

use crate::FaceSetup;

#[cfg(feature = "alvr")]
use self::alvr::AlvrReceiver;

#[cfg(feature = "babble")]
use self::babble::BabbleEtvrReceiver;

#[cfg(feature = "openxr")]
use self::openxr::OpenXrReceiver;

use self::unified::{CombinedExpression, UnifiedExpressions, UnifiedTrackingData, NUM_SHAPES};

use super::{
    ext_oscjson::{MysteryParam, OscJsonNode},
    AppState,
};

use strum::EnumCount;
use strum::IntoEnumIterator;

#[cfg(feature = "alvr")]
mod alvr;
#[cfg(feature = "babble")]
mod babble;
mod face2_fb;
#[cfg(feature = "openxr")]
mod htc;
#[cfg(feature = "openxr")]
mod openxr;
mod sranipal;
pub mod unified;

/// A trait defining the interface for a face tracking data receiver.
/// This allows for different tracking sources (OpenXR, ALVR, etc.) to be used interchangeably.
trait FaceReceiver {
    /// Called once to start the tracking loop or initialize the connection.
    fn start_loop(&mut self);
    /// Called on each frame to receive new tracking data.
    fn receive(&mut self, _data: &mut UnifiedTrackingData, _: &mut AppState);
}

/// A dummy receiver that does nothing. Used when no face tracking is enabled.
struct DummyReceiver;

impl FaceReceiver for DummyReceiver {
    fn start_loop(&mut self) {}
    fn receive(&mut self, _data: &mut UnifiedTrackingData, _: &mut AppState) {}
}

/// The main struct for the tracking extension.
/// It manages the unified tracking data, the mapping to OSC parameters,
/// and the active face tracking receiver.
pub struct ExtTracking {
    /// The unified tracking data structure that holds all face expression values.
    pub data: UnifiedTrackingData,
    /// An array that maps each of the possible face shapes to an OSC parameter configuration.
    params: [Option<MysteryParam>; NUM_SHAPES],
    /// The currently active face tracking receiver, boxed as a trait object.
    receiver: Box<dyn FaceReceiver>,
}

impl ExtTracking {
    /// Creates a new `ExtTracking` instance based on the selected `FaceSetup`.
    pub fn new(setup: FaceSetup) -> Self {
        // A set of default parameters for combined expressions.
        // These are used as a fallback if an avatar's OSC JSON is not available or doesn't define them.
        let default_combined = vec![
            CombinedExpression::BrowExpressionLeft,
            CombinedExpression::BrowExpressionRight,
            CombinedExpression::EyeLidLeft,
            CombinedExpression::EyeLidRight,
            CombinedExpression::JawX,
            CombinedExpression::LipFunnelLower,
            CombinedExpression::LipFunnelUpper,
            CombinedExpression::LipPucker,
            CombinedExpression::MouthLowerDown,
            CombinedExpression::MouthStretchTightenLeft,
            CombinedExpression::MouthStretchTightenRight,
            CombinedExpression::MouthUpperUp,
            CombinedExpression::MouthX,
            CombinedExpression::SmileSadLeft,
            CombinedExpression::SmileSadRight,
        ];
        let default_unified = vec![
            UnifiedExpressions::CheekPuffLeft,
            UnifiedExpressions::CheekPuffRight,
            UnifiedExpressions::EyeSquintLeft,
            UnifiedExpressions::EyeSquintRight,
            UnifiedExpressions::JawOpen,
            UnifiedExpressions::MouthClosed,
        ];

        let mut params = array::from_fn(|_| None);

        // Initialize the params array with default configurations for combined expressions.
        for e in default_combined.into_iter() {
            let name: &str = e.into();
            let new = MysteryParam {
                name: name.into(),
                main_address: Some(format!("FT/v2/{}", name).into()),
                addresses: array::from_fn(|_| None),
                neg_address: None,
                num_bits: 0,
                last_value: 0.,
                last_bits: [false; 8],
            };
            params[e as usize] = Some(new);
        }

        // Initialize the params array with default configurations for unified expressions.
        for e in default_unified.into_iter() {
            let name: &str = e.into();
            let new = MysteryParam {
                name: name.into(),
                main_address: Some(format!("FT/v2/{}", name).into()),
                addresses: array::from_fn(|_| None),
                neg_address: None,
                num_bits: 0,
                last_value: 0.,
                last_bits: [false; 8],
            };
            params[e as usize] = Some(new);
        }

        // Select and instantiate the appropriate face receiver based on the command-line arguments.
        let receiver: Box<dyn FaceReceiver> = match setup {
            FaceSetup::Dummy => Box::new(DummyReceiver {}),
            #[cfg(feature = "alvr")]
            FaceSetup::Alvr => Box::new(AlvrReceiver::new()),
            #[cfg(feature = "openxr")]
            FaceSetup::Openxr => Box::new(OpenXrReceiver::new()),
            #[cfg(feature = "babble")]
            FaceSetup::Babble { listen } => Box::new(BabbleEtvrReceiver::new(listen)),
        };

        let mut me = Self {
            data: UnifiedTrackingData::default(),
            params,
            receiver,
        };

        log::info!("--- Default params ---");
        me.print_params();

        // Start the receiver's loop.
        me.receiver.start_loop();

        me
    }

    /// This method is called on each application tick to process tracking data.
    pub fn step(&mut self, state: &mut AppState, bundle: &mut OscBundle) {
        // Check for various state flags that might inhibit face tracking.
        let motion = matches!(state.params.get("Motion"), Some(OscType::Int(1)));
        let face_override = matches!(state.params.get("FaceFreeze"), Some(OscType::Bool(true)));
        let afk = matches!(state.params.get("AFK"), Some(OscType::Bool(true)))
            || matches!(state.params.get("IsAfk"), Some(OscType::Bool(true)));

        if afk {
            log::debug!("AFK: tracking paused");
        } else if motion ^ face_override {
            // `motion` is an old parameter for freezing the avatar, `FaceFreeze` is the new one.
            // The XOR handles either one being active.
            log::debug!("Freeze: tracking paused");
        } else {
            // If not paused, receive new data and calculate combined expressions.
            self.receiver.receive(&mut self.data, state);
            self.data.calc_combined(state);
        }

        // Another pause mechanism.
        if matches!(state.params.get("FacePause"), Some(OscType::Bool(true))) {
            log::debug!("FacePause: tracking paused");
            return;
        }

        // Apply the final tracking data to the OSC bundle to be sent.
        self.data.apply_to_bundle(&mut self.params, bundle);
    }

    /// Called when a new avatar is loaded to parse its OSC JSON configuration.
    pub fn osc_json(&mut self, avatar_node: &OscJsonNode) {
        // Reset all existing parameter mappings.
        self.params.iter_mut().for_each(|p| *p = None);

        let Some(parameters) = avatar_node.get("parameters") else {
            log::warn!("oscjson: Could not read /avatar/parameters");
            return;
        };

        // Recursively process the parameters node to find face tracking parameters.
        self.process_node_recursive("parameters", parameters);
        self.print_params();
    }

    /// Recursively traverses the OSC JSON node tree to find and configure face tracking parameters.
    fn process_node_recursive(&mut self, name: &str, node: &OscJsonNode) -> Option<()> {
        // Regex to capture the base name of a parameter and its type (e.g., "Negative" or a bit index).
        static FT_PARAMS_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^(.+?)(Negative|\d+)?$").unwrap());

        // If the node has children, recurse into them.
        if let Some(contents) = node.contents.as_ref() {
            log::debug!("Checking {}", name);
            for (name, node) in contents.iter() {
                let _ = self.process_node_recursive(name, node);
            }
            return None;
        }

        // If it's a leaf node, try to match it as a face tracking parameter.
        if let Some(m) = FT_PARAMS_REGEX.captures(name) {
            let main: Arc<str> = m[1].into();

            log::debug!("Param: {}", name);
            // Try to map the parameter name to a known expression enum.
            let idx = UnifiedExpressions::from_str(&main)
                .map(|e| e as usize)
                .or_else(|_| CombinedExpression::from_str(&main).map(|e| e as usize))
                .or_else(|_| SRanipalExpression::from_str(&main).map(|e| e as usize))
                .ok()?;

            log::debug!(
                "Match: {}",
                UnifiedExpressions::iter()
                    .nth(idx)
                    .map(|e| format!("UnifiedExpressions::{:?}", e))
                    .or_else(|| CombinedExpression::iter()
                        .nth(idx - UnifiedExpressions::COUNT)
                        .map(|e| format!("CombinedExpression::{:?}", e)))
                    .or_else(|| Some("None".to_string()))
                    .unwrap()
            );

            let create = self.params[idx].is_none();

            if create {
                let new = MysteryParam {
                    name: main.clone(),
                    main_address: None,
                    addresses: array::from_fn(|_| None),
                    neg_address: None,
                    num_bits: 0,
                    last_value: 0.,
                    last_bits: [false; 8],
                };
                self.params[idx] = Some(new);
            };

            // Update the parameter configuration based on whether it's a negative, binary, or float parameter.
            let stored = self.params[idx].as_mut().unwrap();
            match m.get(2).map(|s| s.as_str()) {
                Some("Negative") => {
                    let addr = &node.full_path.as_ref()[super::PARAM_PREFIX.len()..];
                    stored.neg_address = Some(addr.into());
                }
                Some(digit) => {
                    let digit = digit.parse::<f32>().unwrap();
                    let idx = digit.log2() as usize;
                    let addr = &node.full_path.as_ref()[super::PARAM_PREFIX.len()..];
                    stored.num_bits = stored.num_bits.max(idx + 1);
                    stored.addresses[idx] = Some(addr.into());
                }
                None => {
                    let addr = &node.full_path.as_ref()[super::PARAM_PREFIX.len()..];
                    stored.main_address = Some(addr.into());
                }
            }
        }
        None
    }

    /// Prints the currently configured parameters to the log for debugging.
    fn print_params(&self) {
        for v in self.params.iter().filter_map(|p| p.as_ref()) {
            let mut elems = vec![];

            if v.main_address.is_some() {
                elems.push("float".into())
            }
            if v.num_bits > 0 {
                elems.push(if v.num_bits > 1 {
                    format!("{} bit", v.num_bits)
                } else {
                    format!("{} bits", v.num_bits)
                });
            }
            if v.neg_address.is_some() {
                elems.push("neg".into());
            }
            log::info!("{}: {}", v.name, elems.join(" + "))
        }
    }
}
