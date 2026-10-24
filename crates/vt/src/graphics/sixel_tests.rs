//! Tests of the Sixel decoder, driven directly: bytes in, `DecodedSixel` out.

use super::*;

const RED: [u8; 4] = [255, 0, 0, 255];
const BLUE: [u8; 4] = [0, 0, 255, 255];
const CLEAR: [u8; 4] = TRANSPARENT;

fn decode(input: &[u8]) -> Option<DecodedSixel> {
    let mut decoder = SixelDecoder::new();
    for &byte in input {
        decoder.put(byte);
    }
    decoder.finish()
}

fn image(input: &str) -> DecodedSixel {
    decode(input.as_bytes()).expect("the input paints something")
}

fn pixel(image: &DecodedSixel, x: u32, y: u32) -> [u8; 4] {
    let at = (y as usize * image.width as usize + x as usize) * 4;
    image.rgba[at..at + 4].try_into().expect("four bytes")
}

#[test]
fn minimal_image() {
    let image = image("#0;2;100;0;0#0~");
    assert_eq!((image.width, image.height), (1, 6));
    assert_eq!(image.rgba.len(), 6 * 4);
    for y in 0..6 {
        assert_eq!(pixel(&image, 0, y), RED);
    }
}

#[test]
fn rgb_and_hls_registers() {
    let two = image("#0;2;100;0;0#1;2;0;0;100#0~$#1?~");
    assert_eq!((two.width, two.height), (2, 6));
    assert_eq!(pixel(&two, 0, 0), RED);
    assert_eq!(pixel(&two, 1, 0), BLUE);

    // DEC hue: 0 is blue, 120 red, 240 green.
    assert_eq!(pixel(&image("#0;1;120;50;100#0~"), 0, 0), RED);
    assert_eq!(pixel(&image("#0;1;0;50;100#0~"), 0, 0), BLUE);
    assert_eq!(pixel(&image("#0;1;240;50;100#0~"), 0, 0), [0, 255, 0, 255]);
    // No saturation: grey at the lightness.
    assert_eq!(pixel(&image("#0;1;0;50;0#0~"), 0, 0), [128, 128, 128, 255]);
}

#[test]
fn repeat_and_bands() {
    let repeated = image("#0;2;100;0;0!3A");
    assert_eq!((repeated.width, repeated.height), (3, 6));
    for x in 0..3 {
        assert_eq!(pixel(&repeated, x, 0), CLEAR);
        assert_eq!(pixel(&repeated, x, 1), RED);
        assert_eq!(pixel(&repeated, x, 2), CLEAR);
    }
    // A zero count paints once.
    assert_eq!(image("!0~").width, 1);
    // A repeat that meets no sixel is dropped and the byte is read as itself.
    assert!(decode(b"!5$").is_none());
    let dropped = image("!5$~");
    assert_eq!(dropped.width, 1);
    let bands = image("~-~");
    assert_eq!((bands.width, bands.height), (1, 12));
}

#[test]
fn untouched_pixels_are_transparent() {
    let mut decoder = SixelDecoder::new();
    for byte in b"#0;2;100;0;0!3A" {
        decoder.put(*byte);
    }
    let image = decoder.finish().expect("painted");
    assert_eq!(pixel(&image, 2, 1), RED);
    assert_eq!(pixel(&image, 2, 0), CLEAR);
    assert_eq!(pixel(&image, 0, 0), CLEAR);
}

#[test]
fn raster_wins() {
    let cropped = image("\"1;1;1;3~-~");
    assert_eq!((cropped.width, cropped.height), (1, 3));
    assert_eq!(cropped.band_pixels, 6);
    assert_eq!(image("~-~").height, 12);

    let padded = image("\"1;1;4;20#0;2;100;0;0#0~");
    assert_eq!((padded.width, padded.height), (4, 20));
    assert_eq!(pixel(&padded, 0, 5), RED);
    assert_eq!(pixel(&padded, 1, 5), CLEAR);
    assert_eq!(pixel(&padded, 0, 6), CLEAR);

    // Three arguments, or a zero axis, declare nothing.
    assert_eq!(image("\"1;1;5~").width, 1);
    assert_eq!(image("\"1;1;0;5~").height, 6);
    assert_eq!(image("\"1;1;5;0~").width, 1);
    // The last declaration wins, anywhere in the stream.
    let late = image("~\"1;1;2;2");
    assert_eq!((late.width, late.height), (2, 2));
}

#[test]
fn default_palette_and_register_wrap() {
    assert_eq!(pixel(&image("#2~"), 0, 0), [204, 33, 33, 255]);
    assert_eq!(pixel(&image("#0~"), 0, 0), [0, 0, 0, 255]);
    assert_eq!(pixel(&image("#15~"), 0, 0), [204, 204, 204, 255]);
    assert_eq!(pixel(&image("#16~"), 0, 0), [0, 0, 0, 255]);
    // Register 257 is register 1.
    assert_eq!(pixel(&image("#257;2;100;0;0#1~"), 0, 0), RED);
    // An unknown colour space selects without defining.
    assert_eq!(pixel(&image("#1;7;1;2;3#1~"), 0, 0), [51, 51, 204, 255]);
    // Fewer than five arguments select only.
    assert_eq!(pixel(&image("#1;2;100;0#1~"), 0, 0), [51, 51, 204, 255]);
}

#[test]
fn colour_is_baked_at_paint_time() {
    let image = image("#0;2;100;0;0~#0;2;0;100;0~");
    assert_eq!(image.width, 2);
    assert_eq!(pixel(&image, 0, 0), RED);
    assert_eq!(pixel(&image, 1, 0), [0, 255, 0, 255]);
}

#[test]
fn blank_sixel_extends_size() {
    let blank = image("?");
    assert_eq!((blank.width, blank.height), (1, 6));
    assert!(blank.rgba.iter().all(|byte| *byte == 0));
    assert!(decode(b"").is_none());
    assert!(decode(b"#0").is_none());
    assert!(decode(b"$-").is_none());
}

#[test]
fn band_pixels_counts_bands_above_the_last() {
    assert_eq!(image("~").band_pixels, 0);
    assert_eq!(image("~-~").band_pixels, 6);
    assert_eq!(image("~-~-").band_pixels, 12);
}

#[test]
fn graphics_carriage_return_rewinds_the_band() {
    let image = image("#0;2;100;0;0~~$#1;2;0;0;100?~");
    assert_eq!(image.width, 2);
    assert_eq!(pixel(&image, 0, 0), RED);
    // Blue painted over column 1 only.
    assert_eq!(pixel(&image, 1, 0), BLUE);
}

#[test]
fn bytes_without_a_rule_are_dropped() {
    let image = image(" \t\x01 5;7 ~\u{7f}");
    assert_eq!((image.width, image.height), (1, 6));
}

#[test]
fn repeat_reads_the_byte_after_the_count_in_ground() {
    // `!` then `#`: the repeat is dropped and `#0` selects as usual.
    let image = image("!3#0;2;100;0;0~");
    assert_eq!(image.width, 1);
    assert_eq!(pixel(&image, 0, 0), RED);
}

// ---- hostile input ----

#[test]
fn huge_repeat_stops_at_the_width_cap() {
    let image = image("#0!99999~");
    assert_eq!((image.width, image.height), (4096, 6));
    assert_eq!(image.rgba.len(), 4096 * 6 * 4);
}

#[test]
fn bands_past_the_height_cap_are_dropped() {
    let input: String = "-~".repeat(700);
    let image = image(&input);
    assert_eq!(image.height, 4096);
    assert_eq!(image.band_pixels, 4096);
    assert_eq!(image.rgba.len(), image.width as usize * 4096 * 4);
}

#[test]
fn raster_only_image_is_transparent_at_the_cap() {
    let image = image("\"1;1;99999;99999");
    assert_eq!((image.width, image.height), (4096, 4096));
    assert_eq!(image.rgba.len(), 4096 * 4096 * 4);
    assert!(image.rgba.iter().all(|byte| *byte == 0));
}

#[test]
fn hls_hue_max_does_not_overflow() {
    let image = image("#0;1;4294967295;50;50#0~");
    assert_eq!(pixel(&image, 0, 0)[3], 255);
    // 4294967295 % 360 = 255, the hue the saturated argument stands for.
    assert_eq!(hls_to_rgb(u32::MAX, 50, 50), hls_to_rgb(255, 50, 50));
}

#[test]
fn endless_arguments_are_not_stored() {
    let mut decoder = SixelDecoder::new();
    decoder.put(b'#');
    decoder.put(b'1');
    for index in 0..(1 << 20) {
        decoder.put(if index % 2 == 0 { b';' } else { b'9' });
    }
    assert_eq!(decoder.arg_count, u8::MAX);
    decoder.put(b'~');
    let image = decoder.finish().expect("painted");
    assert_eq!((image.width, image.height), (1, 6));
}

#[test]
fn digits_saturate() {
    let image = decode(b"\"1;1;99999999999999999999;3~").expect("raster");
    assert_eq!((image.width, image.height), (4096, 3));
}

#[test]
fn a_command_open_at_the_end_still_counts() {
    let sized = image("\"1;1;7;9");
    assert_eq!((sized.width, sized.height), (7, 9));
    let sized = image("#3;2;100;0;0\"1;1;2;2");
    assert_eq!((sized.width, sized.height), (2, 2));
}

#[test]
fn random_bytes_stay_within_the_caps() {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut decoder = SixelDecoder::new();
    for _ in 0..20_000 {
        decoder.put(0x20 + (next() % 0x5F) as u8);
    }
    if let Some(image) = decoder.finish() {
        assert!(image.width <= MAX_DIMENSION && image.height <= MAX_DIMENSION);
        assert_eq!(
            image.rgba.len(),
            image.width as usize * image.height as usize * 4
        );
    }
}

#[test]
fn debug_names_the_type_only() {
    assert_eq!(format!("{:?}", SixelDecoder::new()), "SixelDecoder");
}
