use std::{collections::HashMap, f32::consts::PI, ops::Range, sync::Arc};

use colored::{Color, Colorize};
use glam::Vec3;
use log::info;
use once_cell::sync::Lazy;
use rosc::{OscBundle, OscType};

use crate::core::ext_tracking::unified::UnifiedExpressions;

use super::{bundle::AvatarBundle, ext_tracking::ExtTracking, AppState};

// --- Constants for movement thresholds ---
const MOVE_THRESHOLD_METERS: f32 = 0.1;
const RUN_THRESHOLD_METERS: f32 = 0.5;
const ROTATE_THRESHOLD_RAD: f32 = PI / 120.; // 1.5 degrees
const ROTATE_START_THRESHOLD_RAD: f32 = PI * 2.; // A very high value, effectively disabling rotation start based on this threshold.

// --- Status messages for the UI, lazily initialized ---
/// Status message for when "Follow" mode is active.
static STA_FLW: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "FOLLOW".color(Color::Green)).into());
/// Status message for when "Manual" autopilot is active.
static STA_MAN: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "MANUAL".color(Color::Green)).into());
/// Status message for when autopilot is off.
static STA_OFF: Lazy<Arc<str>> =
    Lazy::new(|| format!("{}", "AP-OFF".color(Color::BrightBlack)).into());

/// This struct manages the state for the AutoPilot extension.
/// It allows for controlling the avatar's movement and actions using facial expressions or by following a target.
pub struct ExtAutoPilot {
    voice: bool,       // Is the "Voice" button currently pressed?
    voice_lock: bool,  // A lock to prevent rapid toggling of the voice state.
    jumped: bool,      // Is the "Jump" button currently pressed?
    jump_cd: i32,      // A cooldown timer for the jump action.
    follow_before: bool, // Was the avatar in "Follow" mode in the previous step?
    last_sent: Vec3,   // The last set of movement values sent, to avoid sending redundant OSC messages.
}

impl ExtAutoPilot {
    pub fn new() -> Self {
        Self {
            voice: false,
            voice_lock: false,
            jumped: false,
            jump_cd: 0,
            follow_before: false,
            last_sent: Vec3::ZERO,
        }
    }

    /// The main update loop for the AutoPilot extension, called on every frame.
    /// It decides which control mode to use (Follow, Manual, or Off) and sends the appropriate OSC commands.
    pub fn step(&mut self, state: &mut AppState, tracking: &ExtTracking, bundle: &mut OscBundle) {
        let mut status_set = false;

        // Handle the "avatar flight" mechanic first.
        self.avatar_flight(state, bundle);

        // --- Determine control mode ---
        let mut follow = false;
        let mut follow_distance = MOVE_THRESHOLD_METERS;
        let mut allow_rotate = false;

        // "Follow" mode is activated by grabbing a "Seeker" object or enabling a tracker.
        if let Some(OscType::Bool(true)) = state.params.get("Seeker_IsGrabbed") {
            follow = true;
        } else if let Some(OscType::Bool(true)) = state.params.get("Tracker1_Enable") {
            follow = true;
            allow_rotate = true;
            follow_distance = RUN_THRESHOLD_METERS;
        }

        let mut look_horizontal = 0.;
        let mut vertical = 0.;
        let mut horizontal = 0.;

        if follow {
            // --- Follow Mode Logic ---
            // Calculate movement based on the position of a target object determined by trilateration.
            if let Some(tgt) = vec3_to_target(&state.params) {
                let dist_horizontal = (tgt.x * tgt.x + tgt.z * tgt.z).sqrt();
                let mut theta = (tgt.x / tgt.z).atan(); // Angle to the target

                // Adjust angle based on quadrant
                if tgt.z < 0. {
                    theta = if theta < 0. { PI + theta } else { -PI + theta };
                }

                let abs_theta = theta.abs();

                // If the target is beyond the follow distance, move towards it.
                if dist_horizontal > follow_distance {
                    let mult = (dist_horizontal / RUN_THRESHOLD_METERS).clamp(0., 1.);

                    vertical = tgt.z / dist_horizontal * mult;
                    horizontal = tgt.x / dist_horizontal * mult;
                    if allow_rotate {
                        look_horizontal = theta.signum() * (abs_theta / (PI / 2.)).clamp(0., 1.);
                    }
                    self.follow_before = true;
                } else if allow_rotate && abs_theta > ROTATE_START_THRESHOLD_RAD {
                    // If close to the target, just rotate to face it.
                    look_horizontal = theta.signum() * (abs_theta / (PI / 2.)).clamp(0., 1.);
                }
                state.status.add_item(STA_FLW.clone());
                status_set = true;
            }
        } else if matches!(state.params.get("AutoPilot"), Some(OscType::Bool(true))) {
            // --- Manual Control Logic (using facial expressions) ---
            state.status.add_item(STA_MAN.clone());
            status_set = true;

            // Use eye gaze for looking left/right and jumping.
            if let Some(eye) = tracking.data.eyes[0] {
                if !(-0.6..=0.5).contains(&eye.z) {
                    look_horizontal = -eye.z;
                }

                if eye.y > 0.4 && !self.jumped {
                    bundle.send_input_button("Jump", true);
                    self.jumped = true;
                } else if self.jumped {
                    bundle.send_input_button("Jump", false);
                    self.jumped = false;
                }
            }

            // Use cheek puffing/sucking for forward/backward movement.
            let puff = tracking.data.getu(UnifiedExpressions::CheekPuffLeft)
                + tracking.data.getu(UnifiedExpressions::CheekPuffRight);

            let suck = tracking.data.getu(UnifiedExpressions::CheekSuckLeft)
                + tracking.data.getu(UnifiedExpressions::CheekSuckRight);

            if puff > 0.5 {
                vertical = (puff * 0.6).min(1.0);
            } else if suck > 0.5 {
                vertical = -(suck * 0.6).min(1.0);
            }

            // Use raising eyebrows to toggle the "Voice" button.
            let brows = tracking.data.getu(UnifiedExpressions::BrowInnerUpLeft)
                + tracking.data.getu(UnifiedExpressions::BrowInnerUpRight)
                + tracking.data.getu(UnifiedExpressions::BrowOuterUpLeft)
                + tracking.data.getu(UnifiedExpressions::BrowOuterUpRight);

            if brows < 2.0 {
                self.voice_lock = false; // Release the lock when brows are lowered.
            }

            if brows > 3.0 && !self.voice {
                bundle.send_input_button("Voice", true);
                self.voice = true;
                self.voice_lock = true; // Lock to prevent immediate release.
            } else if self.voice && !self.voice_lock {
                bundle.send_input_button("Voice", false);
                self.voice = false;
            }
        }

        if !status_set {
            state.status.add_item(STA_OFF.clone());
        }

        // --- Send Movement Commands ---
        // Only send updates if the values have changed significantly to reduce network traffic.
        if (look_horizontal - self.last_sent.x).abs() > 0.01 {
            bundle.send_input_axis("LookHorizontal", look_horizontal);
            self.last_sent.x = look_horizontal;
        }

        if (vertical - self.last_sent.y).abs() > 0.01 {
            bundle.send_input_axis("Vertical", vertical);
            self.last_sent.y = vertical;
        }

        if (horizontal - self.last_sent.z).abs() > 0.01 {
            bundle.send_input_axis("Horizontal", horizontal);
            self.last_sent.z = horizontal;
        }
    }

    /// Implements a "flight" or "flap to jump" mechanic.
    /// This is triggered by a specific VRChat emote and raising both hands above the head.
    fn avatar_flight(&mut self, state: &mut AppState, bundle: &mut OscBundle) {
        const FLIGHT_INTS: Range<i32> = 120..125;

        let Some(OscType::Int(emote)) = state.params.get("VRCEmote") else {
            return;
        };

        let left_pos = state.tracking.left_hand.translation;
        let right_pos = state.tracking.right_hand.translation;
        let head_pos = state.tracking.head.translation;

        // If the correct emote is active and hands are above the head...
        if FLIGHT_INTS.contains(emote) && left_pos.y > head_pos.y && right_pos.y > head_pos.y {
            if !self.jumped && self.jump_cd <= 0 {
                // Calculate jump "power" based on hand height.
                let diff = (left_pos.y + left_pos.y) * 0.5 + 0.1 - head_pos.y;
                let diff = diff.clamp(0., 0.3);

                bundle.send_input_button("Jump", true);
                info!("Jumping with diff {}", diff);

                self.jumped = true;
                // Set a cooldown for the next jump, creating a "flap" rhythm.
                self.jump_cd = (30. - 100. * diff) as i32;
            } else {
                bundle.send_input_button("Jump", false);
                self.jump_cd -= 1;
                self.jumped = false;
            }
        } else if self.jumped {
            // Ensure jump is released if conditions are no longer met.
            bundle.send_input_button("Jump", false);
            self.jump_cd = 0;
            self.jumped = false;
        }
    }
}

// --- Trilateration Logic ---
// This section is used to determine the 3D position of a target based on its "contact" distance
// from four known points. This is likely used for the "Follow" mode to track an in-game object.

const CONTACT_RADIUS: f32 = 3.;
const DIST_MULTIPLIER: f32 = 25.;

/// Converts a contact value (0.0 to 1.0) to a distance in meters.
fn contact_to_dist(d: &f32) -> f32 {
    (1. - d) * CONTACT_RADIUS
}

// The four reference points for trilateration.
const P1: Vec3 = Vec3::new(1., 0., 0.);
const P2: Vec3 = Vec3::new(0., 1., 0.);
const P3: Vec3 = Vec3::new(0., 0., 1.);
// The fourth point is implicitly the origin (0,0,0).

/// Calculates the 3D position of a point given its distance from four other known points.
/// See: https://en.wikipedia.org/wiki/Trilateration
fn trilaterate(r1: f32, r2: f32, r3: f32, r4: f32) -> Vec3 {
    let p2_neg_p1 = P2 - P1;
    let p3_neg_p1 = P3 - P1;

    let e_x = p2_neg_p1.normalize();
    let i = e_x.dot(p3_neg_p1);

    let e_y = (p3_neg_p1 - i * e_x).normalize();
    let e_z = e_x.cross(e_y);
    let d = p2_neg_p1.length();
    let j = e_y.dot(p3_neg_p1);

    let r1_sq = r1 * r1;

    let x = (r1_sq - r2 * r2 + d * d) / (2. * d);
    let y = ((r1_sq - r3 * r3 + i * i + j * j) / (2. * j)) - (i / j * x);

    // There are two possible solutions for the z-coordinate.
    let z1 = (r1_sq - x * x - y * y).sqrt();
    let z2 = -1. * z1;

    let ans1 = P1 + x * e_x + y * e_y + z1 * e_z;
    let ans2 = P1 + x * e_x + y * e_y + z2 * e_z;

    // Use the fourth distance (r4) to disambiguate between the two solutions.
    if ans1.length() - r4 < ans2.length() - r4 {
        ans1
    } else {
        ans2
    }
}

/// Reads the four contact parameters from OSC, converts them to distances,
/// and calls the trilateration function to get the final target vector.
fn vec3_to_target(parameters: &HashMap<Arc<str>, OscType>) -> Option<Vec3> {
    let par1 = parameters.get("Seeker_P0")?;
    let par2 = parameters.get("Seeker_P1")?;
    let par3 = parameters.get("Seeker_P2")?;
    let par4 = parameters.get("Seeker_P3")?;

    match (par1, par2, par3, par4) {
        (OscType::Float(c1), OscType::Float(c2), OscType::Float(c3), OscType::Float(c4)) => {
            let r1 = contact_to_dist(c1);
            let r2 = contact_to_dist(c2);
            let r3 = contact_to_dist(c3);
            let r4 = contact_to_dist(c4);
            Some(trilaterate(r1, r2, r3, r4) * DIST_MULTIPLIER)
        }
        _ => None,
    }
}
