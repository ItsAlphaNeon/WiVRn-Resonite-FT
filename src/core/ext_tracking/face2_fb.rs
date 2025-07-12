//! This module handles the conversion of face tracking data from the
//! Facebook `FB_face_tracking2` extension format to the application's
//! `UnifiedExpressions` format. It defines the mapping from the raw
//! blendshape weights provided by the OpenXR extension to the standardized
//! shapes used internally by OscAvMgr.

use super::unified::{UnifiedExpressions, UnifiedShapeAccessors, UnifiedShapes, NUM_SHAPES};

/// Represents the indices of the core face tracking blendshapes provided by the
/// `FB_face_tracking2` extension. The `repr(usize)` allows casting the enum
/// variants directly to indices for accessing the raw float array from the API.
#[allow(non_snake_case, unused)]
#[repr(usize)]
enum FaceFb {
    BrowLowererL,
    BrowLowererR,
    CheekPuffL,
    CheekPuffR,
    CheekRaiserL,
    CheekRaiserR,
    CheekSuckL,
    CheekSuckR,
    ChinRaiserB,
    ChinRaiserT,
    DimplerL,
    DimplerR,
    EyesClosedL,
    EyesClosedR,
    EyesLookDownL,
    EyesLookDownR,
    EyesLookLeftL,
    EyesLookLeftR,
    EyesLookRightL,
    EyesLookRightR,
    EyesLookUpL,
    EyesLookUpR,
    InnerBrowRaiserL,
    InnerBrowRaiserR,
    JawDrop,
    JawSidewaysLeft,
    JawSidewaysRight,
    JawThrust,
    LidTightenerL,
    LidTightenerR,
    LipCornerDepressorL,
    LipCornerDepressorR,
    LipCornerPullerL,
    LipCornerPullerR,
    LipFunnelerLB,
    LipFunnelerLT,
    LipFunnelerRB,
    LipFunnelerRT,
    LipPressorL,
    LipPressorR,
    LipPuckerL,
    LipPuckerR,
    LipStretcherL,
    LipStretcherR,
    LipSuckLB,
    LipSuckLT,
    LipSuckRB,
    LipSuckRT,
    LipTightenerL,
    LipTightenerR,
    LipsToward,
    LowerLipDepressorL,
    LowerLipDepressorR,
    MouthLeft,
    MouthRight,
    NoseWrinklerL,
    NoseWrinklerR,
    OuterBrowRaiserL,
    OuterBrowRaiserR,
    UpperLidRaiserL,
    UpperLidRaiserR,
    UpperLipRaiserL,
    UpperLipRaiserR,
    Max,
}

/// Represents the indices for the extended set of face tracking blendshapes,
/// specifically for tongue tracking, as provided by `FB_face_tracking2`.
/// These indices start after the core set.
#[allow(non_snake_case, unused)]
#[repr(usize)]
enum Face2Fb {
    TongueTipInterdental = 63,
    TongueTipAlveolar,
    TongueFrontDorsalPalate,
    TongueMidDorsalPalate,
    TongueBackDorsalPalate,
    TongueOut,
    TongueRetreat,
    Max,
}

/// Converts a slice of f32 values from the `FB_face_tracking2` extension
/// into the application's `UnifiedShapes` format.
///
/// # Arguments
///
/// * `face_fb` - A slice of f32 containing the raw blendshape weights from the tracker.
///
/// # Returns
///
/// An `Option<UnifiedShapes>` containing the converted data, or `None` if the
/// input slice is too short.
pub(crate) fn face2_fb_to_unified(face_fb: &[f32]) -> Option<UnifiedShapes> {
    let mut shapes: UnifiedShapes = [0.0; NUM_SHAPES];
    // Ensure the input data is long enough to contain all the expected blendshapes.
    if face_fb.len() < FaceFb::Max as usize {
        log::warn!(
            "Face tracking data is too short: {} < {}",
            face_fb.len(),
            FaceFb::Max as usize
        );
        return None;
    }

    // Helper closure to get a blendshape value by its enum variant.
    let getf = |index| face_fb[index as usize];
    // Helper for the extended (Face2Fb) blendshapes.
    let getf2 = |index| face_fb[index as usize];

    // --- Eye Tracking ---
    // Combine left/right look values into a single axis.
    shapes.setu(
        UnifiedExpressions::EyeRightX,
        getf(FaceFb::EyesLookRightR) - getf(FaceFb::EyesLookLeftR),
    );
    shapes.setu(
        UnifiedExpressions::EyeLeftX,
        getf(FaceFb::EyesLookRightL) - getf(FaceFb::EyesLookLeftL),
    );
    // Combine up/down look values into a single axis.
    shapes.setu(
        UnifiedExpressions::EyeY,
        getf(FaceFb::EyesLookUpR) - getf(FaceFb::EyesLookDownR),
    );

    shapes.setu(UnifiedExpressions::EyeClosedLeft, getf(FaceFb::EyesClosedL));
    shapes.setu(
        UnifiedExpressions::EyeClosedRight,
        getf(FaceFb::EyesClosedR),
    );

    // EyeSquint is derived by subtracting the closed amount from the lid tightener.
    shapes.setu(
        UnifiedExpressions::EyeSquintRight,
        getf(FaceFb::LidTightenerR) - getf(FaceFb::EyesClosedR),
    );
    shapes.setu(
        UnifiedExpressions::EyeSquintLeft,
        getf(FaceFb::LidTightenerL) - getf(FaceFb::EyesClosedL),
    );
    shapes.setu(
        UnifiedExpressions::EyeWideRight,
        getf(FaceFb::UpperLidRaiserR),
    );
    shapes.setu(
        UnifiedExpressions::EyeWideLeft,
        getf(FaceFb::UpperLidRaiserL),
    );

    // --- Brow Tracking ---
    shapes.setu(
        UnifiedExpressions::BrowPinchRight,
        getf(FaceFb::BrowLowererR),
    );
    shapes.setu(
        UnifiedExpressions::BrowPinchLeft,
        getf(FaceFb::BrowLowererL),
    );
    shapes.setu(
        UnifiedExpressions::BrowLowererRight,
        getf(FaceFb::BrowLowererR),
    );
    shapes.setu(
        UnifiedExpressions::BrowLowererLeft,
        getf(FaceFb::BrowLowererL),
    );
    shapes.setu(
        UnifiedExpressions::BrowInnerUpRight,
        getf(FaceFb::InnerBrowRaiserR),
    );
    shapes.setu(
        UnifiedExpressions::BrowInnerUpLeft,
        getf(FaceFb::InnerBrowRaiserL),
    );
    shapes.setu(
        UnifiedExpressions::BrowOuterUpRight,
        getf(FaceFb::OuterBrowRaiserR),
    );
    shapes.setu(
        UnifiedExpressions::BrowOuterUpLeft,
        getf(FaceFb::OuterBrowRaiserL),
    );

    // --- Cheek Tracking ---
    shapes.setu(
        UnifiedExpressions::CheekSquintRight,
        getf(FaceFb::CheekRaiserR),
    );
    shapes.setu(
        UnifiedExpressions::CheekSquintLeft,
        getf(FaceFb::CheekRaiserL),
    );
    shapes.setu(UnifiedExpressions::CheekPuffRight, getf(FaceFb::CheekPuffR));
    shapes.setu(UnifiedExpressions::CheekPuffLeft, getf(FaceFb::CheekPuffL));
    shapes.setu(UnifiedExpressions::CheekSuckRight, getf(FaceFb::CheekSuckR));
    shapes.setu(UnifiedExpressions::CheekSuckLeft, getf(FaceFb::CheekSuckL));

    // --- Jaw and Mouth Tracking ---
    shapes.setu(UnifiedExpressions::JawOpen, getf(FaceFb::JawDrop));
    shapes.setu(UnifiedExpressions::JawRight, getf(FaceFb::JawSidewaysRight));
    shapes.setu(UnifiedExpressions::JawLeft, getf(FaceFb::JawSidewaysLeft));
    shapes.setu(UnifiedExpressions::JawForward, getf(FaceFb::JawThrust));
    shapes.setu(UnifiedExpressions::MouthClosed, getf(FaceFb::LipsToward));

    // --- Lip Suck and Funnel ---
    // LipSuck is derived from a combination of LipSuck and inverted UpperLipRaiser.
    shapes.setu(
        UnifiedExpressions::LipSuckUpperRight,
        (1.0 - getf(FaceFb::UpperLipRaiserR).powf(0.1666)).min(getf(FaceFb::LipSuckRT)),
    );
    shapes.setu(
        UnifiedExpressions::LipSuckUpperLeft,
        (1.0 - getf(FaceFb::UpperLipRaiserL).powf(0.1666)).min(getf(FaceFb::LipSuckLT)),
    );

    shapes.setu(
        UnifiedExpressions::LipSuckLowerRight,
        getf(FaceFb::LipSuckRB),
    );
    shapes.setu(
        UnifiedExpressions::LipSuckLowerLeft,
        getf(FaceFb::LipSuckLB),
    );
    shapes.setu(
        UnifiedExpressions::LipFunnelUpperRight,
        getf(FaceFb::LipFunnelerRT),
    );
    shapes.setu(
        UnifiedExpressions::LipFunnelUpperLeft,
        getf(FaceFb::LipFunnelerLT),
    );
    shapes.setu(
        UnifiedExpressions::LipFunnelLowerRight,
        getf(FaceFb::LipFunnelerRB),
    );
    shapes.setu(
        UnifiedExpressions::LipFunnelLowerLeft,
        getf(FaceFb::LipFunnelerLB),
    );
    shapes.setu(
        UnifiedExpressions::LipPuckerUpperRight,
        getf(FaceFb::LipPuckerR),
    );
    shapes.setu(
        UnifiedExpressions::LipPuckerUpperLeft,
        getf(FaceFb::LipPuckerL),
    );
    shapes.setu(
        UnifiedExpressions::LipPuckerLowerRight,
        getf(FaceFb::LipPuckerR),
    );
    shapes.setu(
        UnifiedExpressions::LipPuckerLowerLeft,
        getf(FaceFb::LipPuckerL),
    );

    shapes.setu(
        UnifiedExpressions::NoseSneerRight,
        getf(FaceFb::NoseWrinklerR),
    );
    shapes.setu(
        UnifiedExpressions::NoseSneerLeft,
        getf(FaceFb::NoseWrinklerL),
    );

    shapes.setu(
        UnifiedExpressions::MouthLowerDownRight,
        getf(FaceFb::LowerLipDepressorR),
    );
    shapes.setu(
        UnifiedExpressions::MouthLowerDownLeft,
        getf(FaceFb::LowerLipDepressorL),
    );

    // --- Mouth Upper Lip Movement ---
    let mouth_upper_up_right = getf(FaceFb::UpperLipRaiserR);
    let mouth_upper_up_left = getf(FaceFb::UpperLipRaiserL);

    shapes.setu(UnifiedExpressions::MouthUpperUpRight, mouth_upper_up_right);
    shapes.setu(UnifiedExpressions::MouthUpperUpLeft, mouth_upper_up_left);
    // MouthUpperDeepen is mapped directly from the upper lip raiser.
    shapes.setu(
        UnifiedExpressions::MouthUpperDeepenRight,
        mouth_upper_up_right,
    );
    shapes.setu(
        UnifiedExpressions::MouthUpperDeepenLeft,
        mouth_upper_up_left,
    );

    // --- Mouth Horizontal Movement ---
    shapes.setu(
        UnifiedExpressions::MouthUpperRight,
        getf(FaceFb::MouthRight),
    );
    shapes.setu(UnifiedExpressions::MouthUpperLeft, getf(FaceFb::MouthLeft));
    shapes.setu(
        UnifiedExpressions::MouthLowerRight,
        getf(FaceFb::MouthRight),
    );
    shapes.setu(UnifiedExpressions::MouthLowerLeft, getf(FaceFb::MouthLeft));

    // --- Mouth Corner and Slant ---
    shapes.setu(
        UnifiedExpressions::MouthCornerPullRight,
        getf(FaceFb::LipCornerPullerR),
    );
    shapes.setu(
        UnifiedExpressions::MouthCornerPullLeft,
        getf(FaceFb::LipCornerPullerL),
    );
    shapes.setu(
        UnifiedExpressions::MouthCornerSlantRight,
        getf(FaceFb::LipCornerPullerR),
    );
    shapes.setu(
        UnifiedExpressions::MouthCornerSlantLeft,
        getf(FaceFb::LipCornerPullerL),
    );

    // --- Mouth Frown and Stretch ---
    shapes.setu(
        UnifiedExpressions::MouthFrownRight,
        getf(FaceFb::LipCornerDepressorR),
    );
    shapes.setu(
        UnifiedExpressions::MouthFrownLeft,
        getf(FaceFb::LipCornerDepressorL),
    );
    shapes.setu(
        UnifiedExpressions::MouthStretchRight,
        getf(FaceFb::LipStretcherR),
    );
    shapes.setu(
        UnifiedExpressions::MouthStretchLeft,
        getf(FaceFb::LipStretcherL),
    );

    // --- Mouth Dimples and Raisers ---
    // Dimple values are amplified.
    shapes.setu(
        UnifiedExpressions::MouthDimpleLeft,
        (getf(FaceFb::DimplerL) * 2.0).min(1.0),
    );
    shapes.setu(
        UnifiedExpressions::MouthDimpleRight,
        (getf(FaceFb::DimplerR) * 2.0).min(1.0),
    );

    shapes.setu(
        UnifiedExpressions::MouthRaiserUpper,
        getf(FaceFb::ChinRaiserT),
    );
    shapes.setu(
        UnifiedExpressions::MouthRaiserLower,
        getf(FaceFb::ChinRaiserB),
    );
    shapes.setu(
        UnifiedExpressions::MouthPressRight,
        getf(FaceFb::LipPressorR),
    );
    shapes.setu(
        UnifiedExpressions::MouthPressLeft,
        getf(FaceFb::LipPressorL),
    );
    shapes.setu(
        UnifiedExpressions::MouthTightenerRight,
        getf(FaceFb::LipTightenerR),
    );
    shapes.setu(
        UnifiedExpressions::MouthTightenerLeft,
        getf(FaceFb::LipTightenerL),
    );

    // --- Tongue Tracking (if available) ---
    // Check if the extended blendshape data is present.
    if face_fb.len() >= Face2Fb::Max as usize {
        shapes.setu(UnifiedExpressions::TongueOut, getf2(Face2Fb::TongueOut));
        shapes.setu(
            UnifiedExpressions::TongueCurlUp,
            getf2(Face2Fb::TongueTipAlveolar),
        );
    }

    Some(shapes)
}
