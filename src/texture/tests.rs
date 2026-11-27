use super::{
    EmbeddedImage, Encoding, Rgba8Options, Texture, TextureMeta, VTex, VTexFormat, embedded_image,
};

fn fixture(
    [width, height, depth]: [u16; 3],
    format: VTexFormat,
    flags: u16,
    mips: &[&[u8]],
    sheet: &[u8],
) -> Vec<u8> {
    let mut data = vec![0; 40];
    data[..2].copy_from_slice(&1u16.to_le_bytes());
    data[2..4].copy_from_slice(&flags.to_le_bytes());
    for (target, side) in data[20..26]
        .as_chunks_mut::<2>()
        .0
        .iter_mut()
        .zip([width, height, depth])
    {
        *target = side.to_le_bytes();
    }
    data[26] = format.id();
    data[27] = mips.len() as u8;
    if !sheet.is_empty() {
        data[32..36].copy_from_slice(&8u32.to_le_bytes());
        data[36..40].copy_from_slice(&1u32.to_le_bytes());
        for value in [2, 8, sheet.len() as u32] {
            data.extend(value.to_le_bytes());
        }
        data.extend(sheet);
    }

    let mut file = vec![0; 28];
    let resource_size = (file.len() + data.len()) as u32;
    file[..4].copy_from_slice(&resource_size.to_le_bytes());
    file[4..6].copy_from_slice(&12u16.to_le_bytes());
    file[8..12].copy_from_slice(&8u32.to_le_bytes());
    file[12..16].copy_from_slice(&1u32.to_le_bytes());
    file[16..20].copy_from_slice(b"DATA");
    file[20..24].copy_from_slice(&8u32.to_le_bytes());
    file[24..28].copy_from_slice(&(data.len() as u32).to_le_bytes());
    file.extend(data);
    for mip in mips.iter().rev() {
        file.extend_from_slice(mip);
    }
    file
}

#[test]
fn volume_depth_halves_in_authored_mip_order() {
    let top: Vec<_> = [1; 32].into_iter().chain([2; 32]).collect();
    let file = fixture(
        [4, 2, 2],
        VTexFormat::Rgba8888,
        0x220,
        &[&top, &[3; 8], &[4; 4]],
        &[],
    );
    let texture = VTex::parse(&file).unwrap();
    assert!(texture.meta().flags.is_volume());
    assert_eq!(texture.meta().mip_count, 3);
    assert_eq!(texture.meta().encoding_hint(), Some(Encoding::Linear));
    let mip = texture.mip(0).unwrap();
    assert_eq!(mip.dimensions(), [4, 2, 2]);
    assert_eq!(mip.image(1, 0), Some([2; 32].as_slice()));
    let mip = texture.mip(1).unwrap();
    assert_eq!(mip.dimensions(), [2, 1, 1]);
    assert_eq!(mip.data(), &[3; 8]);
    assert!(mip.image(1, 0).is_none());
    assert_eq!(texture.mip(2).unwrap().data(), &[4; 4]);
    assert!(texture.mip(3).is_none());
}

#[test]
fn rgba16f_cube_preserves_face_order_and_expands_hdr_values() {
    let backing: Vec<_> = (0..6u16)
        .flat_map(|face| [0x4000 + face, 0xC400, 1, 0x3800])
        .flat_map(u16::to_le_bytes)
        .collect();
    let file = fixture([1, 1, 1], VTexFormat::Rgba16161616f, 0x10, &[&backing], &[]);
    let texture = VTex::parse(&file).unwrap();
    assert!(texture.meta().flags.is_cube());
    let mip = texture.mip(0).unwrap();
    assert_eq!(mip.image(0, 4), backing.get(32..40));
    assert!(mip.image(0, 6).is_none());
    assert_eq!(
        mip.decode_linear_rgba32f(0, 0, None).unwrap(),
        [[2.0, -4.0, 2.0f32.powi(-24), 0.5]]
    );
    assert_eq!(
        mip.decode_rgba8(0, 0, Rgba8Options::default()).unwrap(),
        [255, 0, 0, 128]
    );
}

#[test]
fn bc6h_full_precision_mode_decodes_reversed_endpoint_bits() {
    let block = [
        79, 54, 41, 158, 30, 206, 89, 170, 3, 16, 136, 215, 16, 177, 46, 191,
    ];
    let decoded = super::bc6::block(&block);
    assert_eq!(decoded[0], [0x1BF2, 0x1957, 0x54EA]);
    assert_eq!(decoded[15], [0x1BF3, 0x1957, 0x54EB]);
    let file = fixture([4, 4, 1], VTexFormat::Bc6h, 0, &[&block], &[]);
    let texture = VTex::parse(&file).unwrap();
    let mip = texture.mip(0).unwrap();
    let linear = mip.decode_linear_rgba32f(0, 0, None).unwrap();
    assert_eq!(linear[0][2], 78.625);
    assert_eq!(
        mip.decode_rgba8(0, 0, Rgba8Options::default()).unwrap()[2],
        255
    );
}

#[test]
fn bc5_preserves_xy_and_can_reconstruct_positive_z() {
    let block = [128, 0, 0, 0, 0, 0, 0, 0, 128, 0, 0, 0, 0, 0, 0, 0];
    let file = fixture([2, 3, 1], VTexFormat::Ati2n, 0, &[&block], &[]);
    let texture = VTex::parse(&file).unwrap();
    let mip = texture.mip(0).unwrap();
    assert_eq!(
        mip.decode_rgba8(0, 0, Rgba8Options::default()).unwrap(),
        [128, 128, 0, 255].repeat(6)
    );
    let options = Rgba8Options {
        reconstruct_normal_z: true,
        ..Rgba8Options::default()
    };
    assert_eq!(
        mip.decode_rgba8(0, 0, options).unwrap(),
        [128, 128, 255, 255].repeat(6)
    );
}

#[test]
fn bc1_transparency_and_bc3_four_color_interpolation_use_distinct_palettes() {
    let color = [0, 0, 0, 0xF8, 0xFF, 0xFF, 0xFF, 0xFF];
    let bc1 = VTex::parse(&fixture([1, 1, 1], VTexFormat::Dxt1, 0, &[&color], &[])).unwrap();
    let options = Rgba8Options::default();
    assert_eq!(
        bc1.mip(0).unwrap().decode_rgba8(0, 0, options).unwrap(),
        [0; 4]
    );
    let bc3 = [128, 0, 0, 0, 0, 0, 0, 0]
        .into_iter()
        .chain(color)
        .collect::<Vec<_>>();
    let bc3 = VTex::parse(&fixture([1, 1, 1], VTexFormat::Dxt5, 0, &[&bc3], &[])).unwrap();
    assert_eq!(
        bc3.mip(0).unwrap().decode_rgba8(0, 0, options).unwrap(),
        [170, 0, 0, 128]
    );
}

#[test]
fn rgba8_converts_rgb_transfer_and_preserves_alpha() {
    let file = fixture(
        [1, 1, 1],
        VTexFormat::Rgba8888,
        0,
        &[&[128, 128, 128, 128]],
        &[],
    );
    let texture = VTex::parse(&file).unwrap();
    let mip = texture.mip(0).unwrap();
    let options = Rgba8Options {
        output_encoding: Some(Encoding::Linear),
        ..Rgba8Options::default()
    };
    assert_eq!(
        mip.decode_rgba8(0, 0, options).unwrap_err().to_string(),
        "vtex: texture source encoding is unspecified"
    );
    let options = Rgba8Options {
        source_encoding: Some(Encoding::Srgb),
        ..options
    };
    assert_eq!(mip.decode_rgba8(0, 0, options).unwrap(), [55, 55, 55, 128]);
}

#[test]
fn sprite_sheet_keeps_all_image_rectangles_in_frame_order() {
    let mut sheet: Vec<_> = [
        8u32,
        1,
        17,
        1,
        24,
        1,
        2.0f32.to_bits(),
        0,
        0,
        0,
        1.0f32.to_bits(),
        8,
        2,
    ]
    .into_iter()
    .flat_map(u32::to_le_bytes)
    .collect();
    let images = [[0.0f32, 0.25, 0.5, 1.0], [0.5, 0.0, 1.0, 1.0]];
    for rect in images {
        sheet.extend(
            rect.into_iter()
                .chain([0.0, 0.0, 1.0, 1.0])
                .flat_map(f32::to_le_bytes),
        );
    }
    let file = fixture([4, 4, 1], VTexFormat::Rgba8888, 0, &[&[0; 64]], &sheet);
    let metadata = TextureMeta::read(&file).unwrap();
    let sheet = metadata.sheet.unwrap();
    let sequence = &sheet.sequences[0];
    assert_eq!(sequence.id, 17);
    assert!(sequence.clamp);
    assert_eq!(sequence.total_time, 2.0);
    for (frame, expected) in sequence.frames[0].images.iter().zip(images) {
        assert_eq!(frame.cropped, expected);
    }
    assert_eq!(sequence.frames[0].images.len(), 2);
    assert_eq!(
        Texture::parse(&file).unwrap().first_frame_rect(),
        Some([0, 1, 2, 4])
    );
}

#[test]
fn eac_keeps_high_index_bits_and_eleven_bit_precision() {
    // Table 13, multiplier 2 and selector 3 at the first texel only.
    let channel = [103, 0x2D, 3 << 5, 0, 0, 0, 0, 0];
    let file = fixture([4, 4, 1], VTexFormat::R11Eac, 0, &[&channel], &[]);
    let texture = VTex::parse(&file).unwrap();
    let mip = texture.mip(0).unwrap();
    let linear = mip.decode_linear_rgba32f(0, 0, None).unwrap();
    assert_eq!(linear[0], [668.0 / 2047.0, 0.0, 0.0, 1.0]);
    assert_eq!(linear[15], [812.0 / 2047.0, 0.0, 0.0, 1.0]);
}

#[test]
fn jpeg_wrapper_preserves_the_encoded_stream() {
    let jpeg = [0xFF, 0xD8, 0xFF, 0xD9];
    let file = fixture([1, 1, 1], VTexFormat::JpegDxt5, 0, &[&jpeg], &[]);
    assert_eq!(
        TextureMeta::read(&file).unwrap().format,
        VTexFormat::JpegDxt5
    );
    assert_eq!(
        embedded_image(&file).unwrap(),
        Some(EmbeddedImage::Jpeg(&jpeg))
    );
    assert_eq!(
        VTex::parse(&file).unwrap_err().to_string(),
        "vtex: unsupported format id 17"
    );
}

#[test]
fn png_16_bit_samples_keep_precision_in_the_native_texture() {
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, 2, 1);
    encoder.set_color(png::ColorType::GrayscaleAlpha);
    encoder.set_depth(png::BitDepth::Sixteen);
    let mut writer = encoder.write_header().unwrap();
    writer
        .write_image_data(&[0x12, 0x34, 0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF])
        .unwrap();
    writer.finish().unwrap();

    let file = fixture([2, 1, 1], VTexFormat::PngRgba8888, 0, &[&png], &[]);
    let native = VTex::parse(&file).unwrap();
    assert_eq!(native.pixel_format(), super::PixelFormat::Rgba16);
    assert_eq!(
        native.mip(0).unwrap().data(),
        &[
            0x34, 0x12, 0x34, 0x12, 0x34, 0x12, 0x67, 0x45, 0xAB, 0x89, 0xAB, 0x89, 0xAB, 0x89,
            0xEF, 0xCD,
        ]
    );
    assert_eq!(
        Texture::parse(&file).unwrap().mips()[0],
        [0x12, 0x12, 0x12, 0x45, 0x89, 0x89, 0x89, 0xCD]
    );
}

#[test]
fn array_layers_are_constant_across_mips() {
    let file = fixture(
        [2, 2, 2],
        VTexFormat::Rgba8888,
        0x40,
        &[&[0; 32], &[1, 2, 3, 4, 5, 6, 7, 8]],
        &[],
    );
    let array = VTex::parse(&file).unwrap();
    assert!(array.meta().flags.is_array());
    let mip = array.mip(1).unwrap();
    assert_eq!(mip.dimensions(), [1, 1, 2]);
    assert_eq!(mip.image(1, 0), Some([5, 6, 7, 8].as_slice()));
}

#[test]
fn block_volume_mips_skip_padded_depth_slices() {
    let mut mip_one = [0xEE; 32];
    mip_one[..8].fill(2);
    mip_one[8..16].fill(3);
    let data = fixture(
        [4, 4, 4],
        VTexFormat::Dxt1,
        0x20,
        &[&[1; 32], &mip_one, &[4; 8]],
        &[],
    );
    let texture = VTex::parse(&data).unwrap();
    assert_eq!(texture.mip(0).unwrap().data(), &[1; 32]);
    let mip = texture.mip(1).unwrap();
    assert_eq!(mip.dimensions(), [2, 2, 2]);
    assert_eq!(mip.data().len(), 16);
    assert_eq!(mip.image(1, 0), Some([3; 8].as_slice()));
    assert_eq!(texture.mip(2).unwrap().data(), &[4; 8]);
}
