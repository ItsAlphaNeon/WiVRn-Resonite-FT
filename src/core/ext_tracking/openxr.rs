use std::{
    ops::Add,
    sync::Arc,
    time::{Duration, Instant},
};

use colored::{Color, Colorize};
use glam::{vec3, Affine3A, EulerRot, Quat};
use mint::{Quaternion, Vector3};
use once_cell::sync::Lazy;
use openxr as xr;
use strum::EnumCount;

use crate::core::{AppState, INSTRUCTIONS_END, INSTRUCTIONS_START, TRACK_ON};

use super::{
    htc::{htc_to_unified, HtcFacialData},
    unified::{UnifiedExpressions, UnifiedShapeAccessors, UnifiedTrackingData},
    FaceReceiver,
};

// Static lazy-initialized strings for status indicators.
// These are colored for terminal output to show the status of different tracking features.
static STA_GAZE: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "GAZE".color(Color::Green)).into());
static STA_GAZE_OFF: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "GAZE".color(Color::Red)).into());
static STA_FACE: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "FACE".color(Color::Green)).into());
static STA_FACE_OFF: Lazy<Arc<str>> = Lazy::new(|| format!("{}", "FACE".color(Color::Red)).into());

/// Represents a receiver for OpenXR face tracking data.
/// It holds an optional `XrState` and tracks the last attempt time for initialization,
/// allowing for periodic retries if initialization fails.
pub struct OpenXrReceiver {
    state: Option<XrState>,
    last_attempt: Instant,
}

impl OpenXrReceiver {
    /// Creates a new `OpenXrReceiver` with no initial state.
    pub fn new() -> Self {
        Self {
            state: None,
            last_attempt: Instant::now(),
        }
    }

    /// Tries to initialize the OpenXR state.
    /// If initialization fails, an error is logged.
    fn try_init(&mut self) {
        self.state = XrState::new().map_err(|e| log::error!("XR: {}", e)).ok();
        self.last_attempt = Instant::now();
    }
}

/// Implementation of the `FaceReceiver` trait for `OpenXrReceiver`.
/// This allows it to be used as a source for face tracking data within the application.
impl FaceReceiver for OpenXrReceiver {
    /// Called to start the tracking loop.
    /// It logs instructions and status information to the console and attempts to initialize OpenXR.
    fn start_loop(&mut self) {
        log::info!("{}", *INSTRUCTIONS_START);
        log::info!("");
        log::info!("Using OpenXR (WiVRn/Monado) to provide face data.");
        log::info!(
            "It's normal to see {} if the HMD is not yet connected.",
            "errors".color(Color::Red)
        );
        log::info!("");
        log::info!("Status bar tickers:");
        log::info!("• {} → face data is being received", *STA_FACE);
        log::info!("• {} → eye data is being received", *STA_GAZE);
        log::info!("• {} → head & wrist data is being received", *TRACK_ON);
        log::info!("");
        log::info!("{}", *INSTRUCTIONS_END);
        self.try_init();
    }

    /// Called to receive new tracking data.
    /// If the OpenXR state is not initialized, it periodically tries to re-initialize.
    /// If initialized, it calls the `receive` method of the `XrState` to update the tracking data.
    /// If receiving data fails, the state is reset.
    fn receive(&mut self, data: &mut UnifiedTrackingData, app: &mut AppState) {
        let Some(state) = self.state.as_mut() else {
            // If not initialized, retry every 15 seconds.
            if self.last_attempt.add(Duration::from_secs(15)) < Instant::now() {
                self.try_init();
            }
            // Update status to indicate that tracking is off.
            app.status.add_item(STA_GAZE_OFF.clone());
            app.status.add_item(STA_FACE_OFF.clone());
            return;
        };

        if let Err(e) = state.receive(data, app) {
            log::error!("XR: {}", e);
            self.state = None;
        }
    }
}

/// Holds the entire state for an OpenXR session.
/// This includes the OpenXR instance, session, spaces, actions, and trackers.
pub(super) struct XrState {
    instance: xr::Instance,
    system: xr::SystemId,
    session: xr::Session<xr::Headless>,
    frame_waiter: xr::FrameWaiter,
    frame_stream: xr::FrameStream<xr::Headless>,
    stage_space: xr::Space,
    view_space: xr::Space,
    eye_space: xr::Space,
    aim_spaces: [xr::Space; 2],
    actions: xr::ActionSet,
    eye_action: xr::Action<xr::Posef>,
    aim_actions: [xr::Action<xr::Posef>; 2],
    events: xr::EventDataBuffer,
    session_running: bool,

    // Optional face trackers for different vendor extensions.
    face_tracker_fb: Option<MyFaceTrackerFB>,
    face_tracker_htc: Option<MyFaceTrackerHTC>,

    // Counter for frames where eyes are considered closed, used for blink detection.
    eyes_closed_frames: u32,
}

impl XrState {
    /// Creates a new `XrState` by initializing the OpenXR runtime, session, actions, and spaces.
    /// It also attempts to create face trackers for supported extensions.
    fn new() -> anyhow::Result<Self> {
        let (instance, system) = xr_init()?;

        // Create an action set for the application's actions.
        let actions = instance.create_action_set("oscavmgr", "OscAvMgr", 0)?;

        // Create actions for eye gaze and hand aim poses.
        let eye_action = actions.create_action("eye_gaze", "Eye Gaze", &[])?;
        let aim_actions = [
            actions.create_action("left_aim", "Left Aim", &[])?,
            actions.create_action("right_aim", "Right Aim", &[])?,
        ];

        // Create a headless session, as we are not rendering anything.
        let (session, frame_waiter, frame_stream) =
            unsafe { instance.create_session(system, &xr::headless::SessionCreateInfo {})? };

        // Suggest bindings for a simple controller profile.
        instance.suggest_interaction_profile_bindings(
            instance.string_to_path("/interaction_profiles/khr/simple_controller")?,
            &[
                xr::Binding::new(
                    &aim_actions[0],
                    instance.string_to_path("/user/hand/left/input/aim/pose")?,
                ),
                xr::Binding::new(
                    &aim_actions[1],
                    instance.string_to_path("/user/hand/right/input/aim/pose")?,
                ),
            ],
        )?;

        // Suggest bindings for the eye gaze interaction profile.
        instance.suggest_interaction_profile_bindings(
            instance.string_to_path("/interaction_profiles/ext/eye_gaze_interaction")?,
            &[xr::Binding::new(
                &eye_action,
                instance.string_to_path("/user/eyes_ext/input/gaze_ext/pose")?,
            )],
        )?;

        // Attach the action sets to the session.
        session.attach_action_sets(&[&actions])?;

        // Create reference spaces for tracking.
        let stage_space =
            session.create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)?;

        let view_space =
            session.create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)?;

        // Create spaces for actions.
        let eye_space =
            eye_action.create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)?;

        let aim_spaces = [
            aim_actions[0].create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)?,
            aim_actions[1].create_space(session.clone(), xr::Path::NULL, xr::Posef::IDENTITY)?,
        ];

        let mut me = Self {
            instance,
            system,
            session,
            frame_waiter,
            frame_stream,
            face_tracker_fb: None,
            face_tracker_htc: None,
            stage_space,
            view_space,
            eye_space,
            aim_spaces,
            actions,
            eye_action,
            aim_actions,
            events: xr::EventDataBuffer::new(),
            session_running: false,
            eyes_closed_frames: 0,
        };

        // Attempt to create face trackers, logging info on failure.
        me.face_tracker_fb = MyFaceTrackerFB::new(&me)
            .map_err(|e| log::info!("FB_face_tracking2: {}", e))
            .ok();
        me.face_tracker_htc = MyFaceTrackerHTC::new(&me)
            .map_err(|e| log::info!("HTC_facial_tracking: {}", e))
            .ok();

        Ok(me)
    }

    /// Helper function to load system properties with a specific extension structure.
    /// This is used to query for support of face tracking extensions.
    fn load_properties<T>(&self, next: *mut T) -> xr::Result<()> {
        unsafe {
            let mut p = xr::sys::SystemProperties {
                ty: xr::sys::SystemProperties::TYPE,
                next: next as _,
                ..std::mem::zeroed()
            };
            let res = (self.instance.fp().get_system_properties)(
                self.instance.as_raw(),
                self.system,
                &mut p,
            );
            if res != xr::sys::Result::SUCCESS {
                return Err(res);
            }
            Ok(())
        }
    }

    /// Polls for OpenXR events, syncs actions, and retrieves tracking data.
    /// This is the main loop for updating tracking information each frame.
    fn receive(
        &mut self,
        data: &mut UnifiedTrackingData,
        state: &mut AppState,
    ) -> anyhow::Result<()> {
        // Poll for OpenXR events and handle session state changes.
        while let Some(event) = self.instance.poll_event(&mut self.events)? {
            use xr::Event::*;
            match event {
                SessionStateChanged(e) => match e.state() {
                    xr::SessionState::READY => {
                        // Begin the session when it's ready.
                        self.session
                            .begin(xr::ViewConfigurationType::PRIMARY_STEREO)?;
                        self.session_running = true;
                        log::info!("XrSession started.")
                    }
                    xr::SessionState::STOPPING => {
                        // End the session when it's stopping.
                        self.session.end()?;
                        self.session_running = false;
                        log::warn!("XrSession stopped.")
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        // Bail out if the session is exiting or lost.
                        anyhow::bail!("XR session exiting");
                    }
                    _ => {}
                },
                InstanceLossPending(_) => {
                    anyhow::bail!("XR instance loss pending");
                }
                EventsLost(e) => {
                    log::warn!("lost {} events", e.lost_event_count());
                }
                _ => {}
            }
        }

        if !self.session_running {
            return Ok(());
        }

        // Predict the next frame time for synchronization.
        let next_frame = xr::Time::from_nanos(
            self.instance.now()?.as_nanos()
                + (state.status.last_frame_time.max(0.03334) * 1_000_000_000f32) as i64,
        );

        // Sync actions to get the latest input data.
        self.session.sync_actions(&[(&self.actions).into()])?;

        // Locate the HMD in stage space.
        let hmd_loc = self.view_space.locate(&self.stage_space, next_frame)?;
        if hmd_loc
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID)
        {
            state.tracking.head = to_affine(&hmd_loc);
            state.tracking.last_received = Instant::now();
        } else {
            // If HMD position is not valid (e.g., sleeping), close the avatar's eyes.
            data.shapes.setu(UnifiedExpressions::EyeClosedLeft, 1.0);
            data.shapes.setu(UnifiedExpressions::EyeClosedRight, 1.0);
        }

        // Locate the aim poses for hands.
        let aim_loc = self.aim_spaces[0].locate(&self.stage_space, next_frame)?;
        state.tracking.left_hand = to_affine(&aim_loc);
        let aim_loc = self.aim_spaces[1].locate(&self.stage_space, next_frame)?;
        state.tracking.right_hand = to_affine(&aim_loc);

        // Locate the eye gaze pose relative to the view space.
        let eye_loc = self.eye_space.locate(&self.view_space, next_frame)?;
        if eye_loc.location_flags.contains(
            xr::SpaceLocationFlags::ORIENTATION_VALID | xr::SpaceLocationFlags::ORIENTATION_TRACKED,
        ) {
            let now_q = to_quat(eye_loc.pose.orientation);
            let (y, x, z) = now_q.to_euler(EulerRot::YXZ);

            // Calculate eye closure based on the pitch of the eye rotation.
            let mut eye_closed = ((x.to_degrees() + 5.0) / -55.0).max(0.0);

            // Simple blink detection: if eye rotation changes rapidly, force eyes closed for a few frames.
            if let Some(last) = data.eyes[0] {
                let last_q = Quat::from_euler(EulerRot::YXZ, last.y, last.x, last.z);

                if last_q.angle_between(now_q).to_degrees() > 10.0 {
                    self.eyes_closed_frames = 5;
                }
            }

            if self.eyes_closed_frames > 0 {
                self.eyes_closed_frames -= 1;
                eye_closed = 1.0;
            }

            // Set eye closed shapes and eye rotation data.
            data.shapes
                .setu(UnifiedExpressions::EyeClosedLeft, eye_closed);
            data.shapes
                .setu(UnifiedExpressions::EyeClosedRight, eye_closed);

            data.eyes[0] = Some(vec3(x, y, z));
            data.eyes[1] = data.eyes[0];
            state.status.add_item(STA_GAZE.clone());
        } else {
            state.status.add_item(STA_GAZE_OFF.clone());
        }

        // Get face tracking data from the Facebook extension if available.
        if let Some(face_tracker) = self.face_tracker_fb.as_ref() {
            let mut weights = [0f32; 70];
            let mut confidences = [0f32; 2];

            let is_valid = face_tracker.get_face_expression_weights(
                next_frame,
                &mut weights,
                &mut confidences,
            )?;

            if is_valid {
                if let Some(shapes) = super::face2_fb::face2_fb_to_unified(&weights) {
                    data.shapes[..=UnifiedExpressions::COUNT]
                        .copy_from_slice(&shapes[..=UnifiedExpressions::COUNT]);
                }
                state.status.add_item(STA_FACE.clone());
            } else {
                state.status.add_item(STA_FACE_OFF.clone());
            }
        };

        // Get face tracking data from the HTC extension if available.
        if let Some(face_tracker) = self.face_tracker_htc.as_ref() {
            let htc_data = face_tracker.get_expressions(next_frame);

            if htc_data.eye.is_some() || htc_data.lip.is_some() {
                let shapes = htc_to_unified(&htc_data);
                data.shapes[..=UnifiedExpressions::COUNT]
                    .copy_from_slice(&shapes[..=UnifiedExpressions::COUNT]);
                state.status.add_item(STA_FACE.clone());
            } else {
                state.status.add_item(STA_FACE_OFF.clone());
            }
        }

        Ok(())
    }
}

/// Initializes the OpenXR entry, instance, and system.
/// It enumerates and enables required and optional extensions.
fn xr_init() -> anyhow::Result<(xr::Instance, xr::SystemId)> {
    let entry = xr::Entry::linked();

    let Ok(available_extensions) = entry.enumerate_extensions() else {
        anyhow::bail!("Failed to enumerate OpenXR extensions.");
    };

    // The MND_headless extension is required for running without a graphical context.
    anyhow::ensure!(
        available_extensions.mnd_headless,
        "Missing MND_headless extension."
    );

    let mut enabled_extensions = xr::ExtensionSet::default();
    enabled_extensions.mnd_headless = true;
    enabled_extensions.khr_convert_timespec_time = true;

    // Enable optional extensions if they are available.
    if available_extensions.ext_eye_gaze_interaction {
        enabled_extensions.ext_eye_gaze_interaction = true;
    } else {
        log::warn!("Missing EXT_eye_gaze_interaction extension. Is Monado/WiVRn up to date?");
    }

    if available_extensions.fb_face_tracking2 {
        enabled_extensions.fb_face_tracking2 = true;
    }

    if available_extensions.htc_facial_tracking {
        enabled_extensions.htc_facial_tracking = true;
    }

    // Create the OpenXR instance.
    let Ok(instance) = entry.create_instance(
        &xr::ApplicationInfo {
            api_version: xr::Version::new(1, 0, 0),
            application_name: "oscavmgr",
            application_version: 0,
            engine_name: "oscavmgr",
            engine_version: 0,
        },
        &enabled_extensions,
        &[],
    ) else {
        anyhow::bail!("Failed to create OpenXR instance.");
    };

    let Ok(instance_props) = instance.properties() else {
        anyhow::bail!("Failed to query OpenXR instance properties.");
    };
    log::info!(
        "Using OpenXR runtime: {} {}",
        instance_props.runtime_name,
        instance_props.runtime_version
    );

    // Get the system ID for the HMD.
    let Ok(system) = instance.system(xr::FormFactor::HEAD_MOUNTED_DISPLAY) else {
        anyhow::bail!("Failed to access OpenXR HMD system.");
    };

    Ok((instance, system))
}

/// Wrapper for the Facebook face tracking extension (FB_face_tracking2).
struct MyFaceTrackerFB {
    api: xr::raw::FaceTracking2FB,
    tracker: xr::sys::FaceTracker2FB,
}

impl MyFaceTrackerFB {
    /// Creates a new Facebook face tracker.
    /// It checks for extension support and initializes the tracker.
    pub fn new(xr_state: &XrState) -> anyhow::Result<Self> {
        if xr_state.instance.exts().fb_face_tracking2.is_none() {
            anyhow::bail!("Extension not supported.");
        }

        // Query system properties for face tracking support.
        let mut props = xr::sys::SystemFaceTrackingProperties2FB {
            ty: xr::StructureType::SYSTEM_FACE_TRACKING_PROPERTIES2_FB,
            next: std::ptr::null_mut(),
            supports_visual_face_tracking: xr::sys::Bool32::from_raw(0),
            supports_audio_face_tracking: xr::sys::Bool32::from_raw(0),
        };

        xr_state.load_properties(&mut props)?;

        if props.supports_visual_face_tracking.into_raw() == 0 {
            anyhow::bail!("Unable to provide visual data.");
        }

        // Load the extension's raw API functions.
        let api = unsafe {
            xr::raw::FaceTracking2FB::load(
                xr_state.session.instance().entry(),
                xr_state.session.instance().as_raw(),
            )?
        };

        let mut data_source = xr::sys::FaceTrackingDataSource2FB::VISUAL;

        let info = xr::sys::FaceTrackerCreateInfo2FB {
            ty: xr::StructureType::FACE_TRACKER_CREATE_INFO2_FB,
            next: std::ptr::null(),
            face_expression_set: xr::FaceExpressionSet2FB::DEFAULT,
            requested_data_source_count: 1,
            requested_data_sources: &mut data_source,
        };

        let mut tracker = xr::sys::FaceTracker2FB::default();

        // Create the face tracker.
        let res =
            unsafe { (api.create_face_tracker2)(xr_state.session.as_raw(), &info, &mut tracker) };
        if res.into_raw() != 0 {
            anyhow::bail!("Could not initialize: {:?}", res);
        }

        log::info!("Using FB_face_tracking2 for face.");

        Ok(Self { api, tracker })
    }

    /// Gets the latest face expression weights.
    pub fn get_face_expression_weights(
        &self,
        time: xr::Time,
        weights: &mut [f32],
        confidences: &mut [f32],
    ) -> anyhow::Result<bool> {
        let mut expressions = xr::sys::FaceExpressionWeights2FB {
            ty: xr::StructureType::FACE_EXPRESSION_WEIGHTS2_FB,
            next: std::ptr::null_mut(),
            weight_count: weights.len() as _,
            weights: weights.as_mut_ptr(),
            confidence_count: confidences.len() as _,
            confidences: confidences.as_mut_ptr(),
            is_eye_following_blendshapes_valid: xr::sys::Bool32::from_raw(0),
            is_valid: xr::sys::Bool32::from_raw(0),
            data_source: xr::sys::FaceTrackingDataSource2FB::VISUAL,
            time,
        };

        let info = xr::sys::FaceExpressionInfo2FB {
            ty: xr::StructureType::FACE_EXPRESSION_INFO2_FB,
            next: std::ptr::null(),
            time,
        };

        let res = unsafe {
            (self.api.get_face_expression_weights2)(self.tracker, &info, &mut expressions)
        };
        if res.into_raw() != 0 {
            anyhow::bail!("Failed to get expression weights");
        }

        Ok(expressions.is_valid.into_raw() != 0)
    }
}

impl Drop for MyFaceTrackerFB {
    /// Destroys the face tracker when the struct is dropped.
    fn drop(&mut self) {
        unsafe {
            (self.api.destroy_face_tracker2)(self.tracker);
        }
    }
}

/// Wrapper for the HTC facial tracking extension (HTC_facial_tracking).
pub(super) struct MyFaceTrackerHTC {
    api: xr::raw::FacialTrackingHTC,
    eye_tracker: Option<xr::sys::FacialTrackerHTC>,
    lip_tracker: Option<xr::sys::FacialTrackerHTC>,
}

impl MyFaceTrackerHTC {
    /// Creates a new HTC face tracker.
    /// It checks for extension support and initializes trackers for eye and lip tracking if supported.
    pub fn new(xr_state: &XrState) -> anyhow::Result<Self> {
        if xr_state.instance.exts().htc_facial_tracking.is_none() {
            anyhow::bail!("Extension not supported.");
        }
        // Query system properties for facial tracking support.
        let mut props = xr::sys::SystemFacialTrackingPropertiesHTC {
            ty: xr::StructureType::SYSTEM_FACIAL_TRACKING_PROPERTIES_HTC,
            next: std::ptr::null_mut(),
            support_eye_facial_tracking: xr::sys::Bool32::from_raw(0),
            support_lip_facial_tracking: xr::sys::Bool32::from_raw(0),
        };

        xr_state.load_properties(&mut props)?;

        if props.support_eye_facial_tracking.into_raw()
            + props.support_lip_facial_tracking.into_raw()
            == 0
        {
            anyhow::bail!("Unable to provide lip/eye data.");
        }

        // Load the extension's raw API functions.
        let api = unsafe {
            xr::raw::FacialTrackingHTC::load(
                xr_state.session.instance().entry(),
                xr_state.session.instance().as_raw(),
            )?
        };

        let mut info = xr::sys::FacialTrackerCreateInfoHTC {
            ty: xr::StructureType::FACIAL_TRACKER_CREATE_INFO_HTC,
            next: std::ptr::null(),
            facial_tracking_type: xr::sys::FacialTrackingTypeHTC::EYE_DEFAULT,
        };

        // Create an eye tracker if supported.
        let eye_tracker = if props.support_eye_facial_tracking.into_raw() != 0 {
            let mut eye_tracker = xr::sys::FacialTrackerHTC::default();

            let res = unsafe {
                (api.create_facial_tracker)(xr_state.session.as_raw(), &info, &mut eye_tracker)
            };
            if res.into_raw() != 0 {
                anyhow::bail!("Could not initialize upper face tracker: {:?}", res);
            }
            Some(eye_tracker)
        } else {
            None
        };

        // Create a lip tracker if supported.
        let lip_tracker = if props.support_lip_facial_tracking.into_raw() != 0 {
            info.facial_tracking_type = xr::sys::FacialTrackingTypeHTC::LIP_DEFAULT;

            let mut lip_tracker = xr::sys::FacialTrackerHTC::default();

            let res = unsafe {
                (api.create_facial_tracker)(xr_state.session.as_raw(), &info, &mut lip_tracker)
            };
            if res.into_raw() != 0 {
                anyhow::bail!("Could not initialize lower face tracker: {:?}", res);
            }
            Some(lip_tracker)
        } else {
            None
        };

        log::info!("Using HTC_facial_tracking for face.");

        Ok(Self {
            api,
            eye_tracker,
            lip_tracker,
        })
    }

    /// Internal function to get expression weights from a specific HTC tracker.
    fn get_expressions_internal<const E: usize>(
        &self,
        tracker: xr::sys::FacialTrackerHTC,
        sample_time: xr::Time,
    ) -> Option<[f32; E]> {
        let mut arr = [0f32; E];
        let mut info = xr::sys::FacialExpressionsHTC {
            ty: xr::StructureType::FACIAL_EXPRESSIONS_HTC,
            next: std::ptr::null_mut(),
            sample_time,
            is_active: xr::sys::Bool32::from_raw(0),
            expression_count: arr.len() as _,
            expression_weightings: arr.as_mut_ptr(),
        };

        let res = unsafe { (self.api.get_facial_expressions)(tracker, &mut info) };
        if res.into_raw() != 0 {
            log::error!("Failed to get HTC facial expression weights");
            return None;
        }

        if info.is_active.into_raw() != 0 {
            Some(arr)
        } else {
            None
        }
    }

    /// Gets expressions from both eye and lip trackers.
    pub fn get_expressions(&self, sample_time: xr::Time) -> HtcFacialData {
        HtcFacialData {
            eye: self
                .eye_tracker
                .and_then(|t| self.get_expressions_internal(t, sample_time)),
            lip: self
                .lip_tracker
                .and_then(|t| self.get_expressions_internal(t, sample_time)),
        }
    }
}

impl Drop for MyFaceTrackerHTC {
    /// Destroys the eye and lip trackers when the struct is dropped.
    fn drop(&mut self) {
        unsafe {
            if let Some(tracker) = self.eye_tracker.take() {
                (self.api.destroy_facial_tracker)(tracker);
            }
            if let Some(tracker) = self.lip_tracker.take() {
                (self.api.destroy_facial_tracker)(tracker);
            }
        }
    }
}

/// Converts an `xr::Quaternionf` to a `glam::Quat`.
fn to_quat(p: xr::Quaternionf) -> Quat {
    let q: Quaternion<f32> = p.into();
    q.into()
}

/// Converts an `xr::SpaceLocation` to a `glam::Affine3A` transformation matrix.
fn to_affine(loc: &xr::SpaceLocation) -> Affine3A {
    let t: Vector3<f32> = loc.pose.position.into();
    Affine3A::from_rotation_translation(to_quat(loc.pose.orientation), t.into())
}
