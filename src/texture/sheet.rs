use crate::budget::Budget;
use crate::bytes::Cursor;
use crate::error::{Result, malformed};

const WHAT: &str = "vtex sprite sheet";
/// Most sequences a sheet has; Dota's have up to a few dozen.
const MAX_SEQUENCES: usize = 1024;
/// Most frames all sequences have together.
const MAX_FRAMES: usize = 16384;

/// A texture's sprite sheet: its animation sequences.
#[derive(Clone, Debug, PartialEq)]
pub struct Sheet {
    /// The sequences, in file order (a particle's sequence number indexes them).
    pub sequences: Vec<Sequence>,
}

/// One animation of a sprite sheet.
#[derive(Clone, Debug, PartialEq)]
pub struct Sequence {
    /// Its id.
    pub id: u32,
    /// It holds its last frame at its end rather than looping.
    pub clamp: bool,
    /// Its frames.
    pub frames: Vec<Frame>,
}

/// One frame of a sequence, its first image's rectangles as texture coordinates
/// `[u0, v0, u1, v1]` (0..1, v down the image like the rows).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    /// How long it shows, as a share of the sequence.
    pub display_time: f32,
    /// The frame cropped to its content.
    pub cropped: [f32; 4],
    /// The whole cell it was cut from.
    pub uncropped: [f32; 4],
}

/// All sequences and images of a Source 2 sprite sheet.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct VTexSheet {
    /// Sequences in file order.
    pub sequences: Vec<VTexSequence>,
}

/// A Source 2 sheet sequence.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct VTexSequence {
    /// Authored sequence ID.
    pub id: u32,
    /// Hold the last frame at the end of the sequence.
    pub clamp: bool,
    /// Authored sequence duration.
    pub total_time: f32,
    /// Frames in file order.
    pub frames: Vec<VTexFrame>,
}

/// A frame with all its image rectangles.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct VTexFrame {
    /// How long it shows, as a share of the sequence.
    pub display_time: f32,
    /// Rectangles for each image in the frame.
    pub images: Vec<FrameImage>,
}

/// Texture coordinates `[u0, v0, u1, v1]`, with v increasing down the image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameImage {
    /// The frame cropped to its content.
    pub cropped: [f32; 4],
    /// The whole cell it was cut from.
    pub uncropped: [f32; 4],
}

impl VTexSheet {
    pub(super) fn into_legacy(self, budget: &mut Budget) -> Result<Sheet> {
        budget.slots::<Sequence>(self.sequences.len())?;
        let sequences = self
            .sequences
            .into_iter()
            .map(|sequence| {
                budget.slots::<Frame>(sequence.frames.len())?;
                let frames = sequence
                    .frames
                    .into_iter()
                    .map(|frame| Frame {
                        display_time: frame.display_time,
                        cropped: frame.images[0].cropped,
                        uncropped: frame.images[0].uncropped,
                    })
                    .collect();
                Ok(Sequence {
                    id: sequence.id,
                    clamp: sequence.clamp,
                    frames,
                })
            })
            .collect::<Result<_>>()?;
        Ok(Sheet { sequences })
    }
}

impl Sheet {
    /// The first frame of the first sequence cropped to its content, if the sheet has a frame.
    pub(super) fn first_rect(&self) -> Option<[f32; 4]> {
        Some(self.sequences.first()?.frames.first()?.cropped)
    }
}

/// The sheet of a texture's sheet block, `None` when it has no sequence.
///
/// The block is an offset to the sequences and their count; a sequence is 32 bytes (id, four
/// flag bytes, an offset to its frames relative to that field, the frame count, the total time,
/// its name and its parameters), a frame 12 bytes (display time, offset to its images, image
/// count) and an image two rectangles of four floats.
pub(super) fn read(data: &[u8], budget: &mut Budget) -> Result<Option<VTexSheet>> {
    let mut head = Cursor::new(data, WHAT);
    let sequences_at = head.u32()? as usize;
    let count = head.u32()? as usize;
    if count == 0 {
        return Ok(None);
    }
    if count > MAX_SEQUENCES {
        return Err(malformed(WHAT, format!("{count} sequences")));
    }
    budget.slots::<VTexSequence>(count)?;
    let mut sequences = Vec::with_capacity(count);
    let mut frames_left = MAX_FRAMES;
    for index in 0..count {
        let at = sequences_at
            .checked_add(index * 32)
            .ok_or_else(|| malformed(WHAT, "offset overflows"))?;
        sequences.push(sequence(data, at, &mut frames_left, budget)?);
    }
    Ok(Some(VTexSheet { sequences }))
}

fn sequence(
    data: &[u8],
    at: usize,
    frames_left: &mut usize,
    budget: &mut Budget,
) -> Result<VTexSequence> {
    let mut seq = cursor(data, at)?;
    let id = seq.u32()?;
    let flags = seq.array::<4>()?;
    let frames_at = relative(at + 8, seq.i32()?)?;
    let count = seq.u32()? as usize;
    let total_time = float(&mut seq)?;
    *frames_left = frames_left
        .checked_sub(count)
        .ok_or_else(|| malformed(WHAT, "more frames than any sheet has"))?;
    budget.slots::<VTexFrame>(count)?;
    let mut frames = Vec::with_capacity(count);
    for index in 0..count {
        let at = frames_at
            .checked_add(index * 12)
            .ok_or_else(|| malformed(WHAT, "offset overflows"))?;
        frames.push(frame(data, at, budget)?);
    }
    Ok(VTexSequence {
        id,
        clamp: flags[0] != 0,
        total_time,
        frames,
    })
}

fn frame(data: &[u8], at: usize, budget: &mut Budget) -> Result<VTexFrame> {
    let mut frame = cursor(data, at)?;
    let display_time = float(&mut frame)?;
    let images_at = relative(at + 4, frame.i32()?)?;
    let count = frame.u32()? as usize;
    let mut image = cursor(data, images_at)?;
    if count == 0 || count > image.remaining() / 32 {
        return Err(malformed(WHAT, format!("{count} images in a frame")));
    }
    budget.slots::<FrameImage>(count)?;
    let mut images = Vec::with_capacity(count);
    for _ in 0..count {
        let mut rects = [[0.0; 4]; 2];
        for value in rects.iter_mut().flatten() {
            *value = float(&mut image)?;
        }
        images.push(FrameImage {
            cropped: rects[0],
            uncropped: rects[1],
        });
    }
    Ok(VTexFrame {
        display_time,
        images,
    })
}

fn float(cursor: &mut Cursor<'_>) -> Result<f32> {
    let value = f32::from_bits(cursor.u32()?);
    if !value.is_finite() {
        return Err(malformed(WHAT, "a value is not a number"));
    }
    Ok(value)
}

/// A reader of `data` from `pos`.
fn cursor(data: &[u8], pos: usize) -> Result<Cursor<'_>> {
    data.get(pos..)
        .map(|rest| Cursor::new(rest, WHAT))
        .ok_or_else(|| malformed(WHAT, format!("offset {pos} runs past the end")))
}

/// The position `delta` bytes from the field at `field`.
fn relative(field: usize, delta: i32) -> Result<usize> {
    field
        .checked_add_signed(delta as isize)
        .ok_or_else(|| malformed(WHAT, "offset overflows"))
}
