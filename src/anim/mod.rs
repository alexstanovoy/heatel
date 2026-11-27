mod clip;
mod layers;
mod segment;
mod sequence;
#[cfg(test)]
mod tests;

pub(crate) use clip::read_clips_with_ctrl;
pub use clip::{
    Clip, Keys, MorphTrack, RootMovement, SourceTracks, Track, read_clips, read_clips_in_place,
};
pub use layers::{
    ActivityEntry, AnimationFlags, AnimationInfo, AnimationSet, AutoLayer, AutoLayerFlags,
    BlendDimensions, BlendGrid, BoneMask, BoneReference, BoneScaleSet, LayeredSequence,
    PoseParameter, SequenceFlags, SequenceReference,
};
pub use sequence::{Activity, AnimEvent, EventData, Sequence, read_sequences};
