use strum::{EnumCount, EnumIter, EnumString, IntoStaticStr};

use super::unified::{CombinedExpression, UnifiedExpressions};

/// Defines a mapping from HTC's SRanipal expression blendshapes to the application's
/// internal `UnifiedExpressions` and `CombinedExpression` enums.
///
/// SRanipal is HTC's SDK for eye and face tracking on devices like the VIVE Pro Eye and VIVE Facial Tracker.
/// This enum ensures that the data received from the SRanipal runtime can be correctly interpreted
/// and translated into the unified tracking model used by this application.
///
/// Each variant of `SRanipalExpression` corresponds to a blendshape provided by the SRanipal SDK.
/// The `#[repr(usize)]` attribute and the explicit casting (`as _` or `as usize`) create a direct
/// numerical mapping to the corresponding variant in the unified expression enums. This allows for
/// efficient lookup and translation of expression data.
#[allow(unused)]
#[repr(usize)]
#[derive(Debug, Clone, Copy, EnumIter, EnumCount, EnumString, IntoStaticStr)]
pub enum SRanipalExpression {
    // --- Eye Tracking ---
    LeftEyeX = UnifiedExpressions::EyeLeftX as _,
    RightEyeX = UnifiedExpressions::EyeRightX as _,
    EyesY = UnifiedExpressions::EyeY as _,

    // --- Eyelid and Squint ---
    EyeLeftWide = UnifiedExpressions::EyeWideLeft as _,
    EyeRightWide = UnifiedExpressions::EyeWideRight as _,
    EyeLeftBlink = UnifiedExpressions::EyeClosedLeft as _,
    EyeRightBlink = UnifiedExpressions::EyeClosedRight as _,
    EyeLeftSqueeze = UnifiedExpressions::EyeSquintLeft as _,
    EyeRightSqueeze = UnifiedExpressions::EyeSquintRight as _,

    // --- Mouth and Lip Shapes ---
    CheekSuck = UnifiedExpressions::CheekSuckLeft as _, // Note: SRanipal might not distinguish left/right for this.
    MouthApeShape = UnifiedExpressions::MouthClosed as _,
    MouthUpperInside = CombinedExpression::LipSuckUpper as usize,
    MouthLowerInside = CombinedExpression::LipSuckLower as usize,
    MouthUpperOverturn = CombinedExpression::LipFunnelUpper as usize,
    MouthLowerOverturn = CombinedExpression::LipFunnelLower as usize,
    MouthPout = CombinedExpression::LipPucker as usize,
    MouthLowerOverlay = UnifiedExpressions::MouthRaiserLower as _,

    // --- Tongue ---
    TongueLongStep1 = UnifiedExpressions::TongueOut as _,

    // The following expressions are commented out because their names in the SRanipal standard
    // are identical to the names used in the `UnifiedExpressions` or `CombinedExpression` enums.
    // This implies that they can be mapped directly by name (using `FromStr`) without needing
    // an explicit entry in this enum. This keeps the mapping cleaner by only defining aliases
    // for expressions where the names differ.
    /* duplicate names
    CheekPuffLeft = UnifiedExpressions::CheekPuffLeft as _,
    CheekPuffRight = UnifiedExpressions::CheekPuffRight as _,
    JawLeft = UnifiedExpressions::JawLeft as _,
    JawRight = UnifiedExpressions::JawRight as _,
    JawForward = UnifiedExpressions::JawForward as _,
    JawOpen = UnifiedExpressions::JawOpen as _,
    MouthSmileLeft = CombinedExpression::MouthSmileLeft as usize,
    MouthSmileRight = CombinedExpression::MouthSmileRight as usize,
    MouthSadLeft = CombinedExpression::MouthSadLeft as usize,
    MouthSadRight = CombinedExpression::MouthSadRight as usize,
    MouthUpperUpLeft = UnifiedExpressions::MouthUpperUpLeft as _,
    MouthUpperUpRight = UnifiedExpressions::MouthUpperUpRight as _,
    MouthLowerDownLeft = UnifiedExpressions::MouthLowerDownLeft as _,
    MouthLowerDownRight = UnifiedExpressions::MouthLowerDownRight as _,
    TongueUp = UnifiedExpressions::TongueUp as _,
    TongueDown = UnifiedExpressions::TongueDown as _,
    TongueLeft = UnifiedExpressions::TongueLeft as _,
    TongueRight = UnifiedExpressions::TongueRight as _,
    TongueRoll = UnifiedExpressions::TongueRoll as _,
    */
}
