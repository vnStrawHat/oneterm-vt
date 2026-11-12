//! Tests of the kitty graphics wire format: control keys, the `APC` reader, base64, decoding.

use miniz_oxide::deflate::compress_to_vec_zlib;

use super::*;

/// Standard base64 with padding.
pub(crate) fn base64(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut text = String::new();
    for chunk in data.chunks(3) {
        let byte = |index: usize| u32::from(chunk.get(index).copied().unwrap_or(0));
        let group = byte(0) << 16 | byte(1) << 8 | byte(2);
        for index in 0..4 {
            if index <= chunk.len() {
                text.push(ALPHABET[(group >> (18 - 6 * index)) as usize & 63] as char);
            } else {
                text.push('=');
            }
        }
    }
    text
}

/// A PNG file of `data` in the given layout; an indexed one gets a red and a half-clear green.
pub(crate) fn png_bytes(
    width: u32,
    height: u32,
    color: png::ColorType,
    depth: png::BitDepth,
    data: &[u8],
) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(color);
    encoder.set_depth(depth);
    if color == png::ColorType::Indexed {
        encoder.set_palette(vec![255, 0, 0, 0, 255, 0]);
        encoder.set_trns(vec![255, 128]);
    }
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(data).unwrap();
    writer.finish().unwrap();
    bytes
}

/// Reads one `APC` string whose body (after the introducer) is `body`.
fn read(state: &mut KittyState, body: &str) -> Outcome {
    state.start(b'_');
    for byte in body.bytes() {
        state.put(byte);
    }
    state.end(false)
}

/// The load of the single command `body`.
fn load(body: &str) -> Load {
    match read(&mut KittyState::default(), body) {
        Outcome::Command(load) => load,
        other => panic!("not a whole command: {other:?}"),
    }
}

fn decode(control: &str, payload: &[u8]) -> Result<Image, Refusal> {
    load(&format!("G{control};{}", base64(payload))).decode()
}

fn code(result: Result<Image, Refusal>) -> &'static str {
    let message = result.unwrap_err().0;
    message.split(':').next().unwrap()
}

#[test]
fn control_keys_are_read() {
    let command = Command::parse(
        b"a=T,q=2,f=100,t=s,o=z,m=1,s=3,v=4,i=5,I=6,p=7,x=8,y=9,w=10,h=11,X=12,Y=13,c=14,r=15,z=-16,C=1,d=Z,U=1,P=2",
    )
    .unwrap();
    let expected = Command {
        action: b'T',
        quiet: 2,
        format: 100,
        medium: b's',
        compression: b'z',
        more: true,
        width: 3,
        height: 4,
        image_id: 5,
        image_number: 6,
        placement_id: 7,
        x: 8,
        y: 9,
        source_width: 10,
        source_height: 11,
        cell_x: 12,
        cell_y: 13,
        cols: 14,
        rows: 15,
        z_index: -16,
        cursor_stays: true,
        delete: b'Z',
        placeholder: true,
        relative: true,
    };
    assert_eq!(command, expected);
}

#[test]
fn absent_keys_take_the_protocol_defaults() {
    let command = Command::parse(b"").unwrap();
    assert_eq!(command, Command::default());
    assert_eq!(
        (
            command.action,
            command.format,
            command.medium,
            command.delete
        ),
        (b't', 32, b'd', b'a')
    );
    assert!(!command.more && !command.cursor_stays && !command.placeholder && !command.relative);
}

#[test]
fn unknown_keys_are_skipped_and_malformed_pairs_refuse_the_command() {
    assert_eq!(Command::parse(b"N=3,a=p,,S=9,").unwrap().action, b'p');
    assert_eq!(Command::parse(b"z=-2147483648").unwrap().z_index, i32::MIN);
    assert_eq!(Command::parse(b"i=4294967295").unwrap().image_id, u32::MAX);
    for malformed in [
        "a",
        "a=",
        "a=tt",
        "=t",
        "i=4294967296",
        "i=-1",
        "i=1x",
        "z=2147483648",
        "z=-",
        "ab=1",
    ] {
        assert_eq!(Command::parse(malformed.as_bytes()), None, "{malformed}");
    }
}

#[test]
fn only_an_apc_that_starts_with_g_is_a_command() {
    let mut state = KittyState::default();
    assert!(matches!(read(&mut state, "Ga=q,i=1"), Outcome::Command(_)));
    assert!(matches!(read(&mut state, "ga=q"), Outcome::Ignored));
    assert!(matches!(read(&mut state, ""), Outcome::Ignored));
    assert!(matches!(read(&mut state, "Ga=tt"), Outcome::Ignored));
    // SOS and PM strings are never commands.
    state.start(b'X');
    "Ga=q".bytes().for_each(|byte| state.put(byte));
    assert!(matches!(state.end(false), Outcome::Ignored));
}

#[test]
fn control_data_past_its_cap_is_no_command() {
    let mut state = KittyState::default();
    let long = format!("Gi=1{}", ",i=1".repeat(200));
    assert!(matches!(read(&mut state, &long), Outcome::Ignored));
    assert!(matches!(read(&mut state, "Ga=q,i=1"), Outcome::Command(_)));
}

#[test]
fn the_payload_is_base64_with_or_without_padding() {
    for length in 0..=10usize {
        let data: Vec<u8> = (0..length).map(|n| (n * 37 + 201) as u8).collect();
        let padded = base64(&data);
        assert_eq!(load(&format!("G;{padded}")).data, data);
        let bare = padded.trim_end_matches('=');
        assert_eq!(load(&format!("G;{bare}")).data, data, "{bare}");
    }
}

#[test]
fn chunks_add_up_and_only_the_first_one_has_keys() {
    let data: Vec<u8> = (0..=255).collect();
    let text = base64(&data);
    let mut state = KittyState::default();
    let first = format!("Ga=T,i=7,m=1;{}", &text[..8]);
    assert!(matches!(read(&mut state, &first), Outcome::Chunk));
    // A padded chunk in the middle, and keys a continuation must not take.
    let middle = format!("Ga=d,i=9,m=1;{}", base64(&data[6..7]));
    assert!(matches!(read(&mut state, &middle), Outcome::Chunk));
    let last = format!("Gm=0;{}", &text[8..]);
    let Outcome::Command(load) = read(&mut state, &last) else {
        panic!("the last chunk ends the load");
    };
    assert_eq!((load.command.action, load.command.image_id), (b'T', 7));
    let mut expected = data[..6].to_vec();
    expected.push(data[6]);
    expected.extend_from_slice(&data[6..]);
    assert_eq!(load.data, expected);
    // The next command starts afresh.
    assert!(matches!(read(&mut state, "Ga=q,i=2"), Outcome::Command(load) if load.data.is_empty()));
}

#[test]
fn an_aborted_chunk_drops_the_load() {
    let mut state = KittyState::default();
    assert!(matches!(
        read(&mut state, "Ga=T,i=7,m=1;AAAA"),
        Outcome::Chunk
    ));
    state.start(b'_');
    "Gm=1;AA".bytes().for_each(|byte| state.put(byte));
    assert!(matches!(state.end(true), Outcome::Ignored));
    let Outcome::Command(load) = read(&mut state, "Ga=q,i=2;AAAA") else {
        panic!("a fresh command");
    };
    assert_eq!((load.command.image_id, load.data.len()), (2, 3));
    // An aborted string that was no command leaves an open load alone.
    assert!(matches!(
        read(&mut state, "Ga=T,i=7,m=1;AAAA"),
        Outcome::Chunk
    ));
    state.start(b'_');
    state.put(b'x');
    assert!(matches!(state.end(true), Outcome::Ignored));
    assert!(
        matches!(read(&mut state, "Gm=0;AAAA"), Outcome::Command(load) if load.data.len() == 6)
    );
}

#[test]
fn a_byte_outside_the_alphabet_spoils_the_payload() {
    let result = load("Ga=q,s=1,v=1,f=24;AA!A").decode();
    assert_eq!(result, Err(Refusal("EINVAL:the payload is not base64")));
}

#[test]
fn a_payload_past_the_cap_is_too_large() {
    let mut big = load("Ga=q,s=1,v=1;AAAAAA==");
    big.too_large = true;
    assert_eq!(code(big.decode()), "EFBIG");
}

#[test]
fn raw_pixels_become_rgba() {
    let rgba = [1, 2, 3, 4, 5, 6, 7, 8];
    let image = decode("s=2,v=1", &rgba).unwrap();
    assert_eq!(
        (image.width, image.height, &image.rgba[..]),
        (2, 1, &rgba[..])
    );
    let image = decode("f=24,s=1,v=2", &[1, 2, 3, 4, 5, 6]).unwrap();
    assert_eq!(image.rgba, [1, 2, 3, 255, 4, 5, 6, 255]);
    // Bytes past the image are ignored.
    let image = decode("f=24,s=1,v=1", &[1, 2, 3, 4, 5, 6]).unwrap();
    assert_eq!(image.rgba, [1, 2, 3, 255]);
}

#[test]
fn raw_pixels_need_a_size_and_enough_bytes() {
    assert_eq!(code(decode("s=2,v=2", &[0; 15])), "ENODATA");
    assert_eq!(code(decode("f=24,s=2,v=2", &[0; 11])), "ENODATA");
    assert_eq!(code(decode("v=1", &[0; 4])), "EINVAL");
    assert_eq!(code(decode("s=1", &[0; 4])), "EINVAL");
    assert_eq!(code(decode("s=4097,v=1", &[0; 4])), "EINVAL");
    assert_eq!(code(decode("s=1,v=4097", &[0; 4])), "EINVAL");
    assert_eq!(code(decode("f=8,s=1,v=1", &[0; 4])), "EINVAL");
    assert_eq!(code(decode("o=x,s=1,v=1", &[0; 4])), "EINVAL");
}

#[test]
fn zlib_payloads_are_inflated_within_the_image_size() {
    let rgba: Vec<u8> = (0..64).collect();
    let image = decode("o=z,s=4,v=4", &compress_to_vec_zlib(&rgba, 6)).unwrap();
    assert_eq!(image.rgba, rgba);
    let image = decode("o=z,f=24,s=1,v=1", &compress_to_vec_zlib(&[9, 8, 7], 6)).unwrap();
    assert_eq!(image.rgba, [9, 8, 7, 255]);
    // A stream that inflates to far more than the image: only the image is kept.
    let bomb = compress_to_vec_zlib(&vec![7; 1 << 20], 6);
    let image = decode("o=z,s=1,v=1", &bomb).unwrap();
    assert_eq!(image.rgba, [7, 7, 7, 7]);
    assert_eq!(
        code(decode("o=z,s=4,v=4", &compress_to_vec_zlib(&rgba[..10], 6))),
        "ENODATA"
    );
    assert_eq!(code(decode("o=z,s=1,v=1", b"not a zlib stream")), "ENODATA");
}

#[test]
fn png_payloads_of_every_layout_become_rgba() {
    use png::BitDepth::{Eight, Sixteen};
    use png::ColorType::{Grayscale, GrayscaleAlpha, Indexed, Rgb, Rgba};
    let cases: [(png::ColorType, png::BitDepth, &[u8], &[u8]); 6] = [
        (
            Rgba,
            Eight,
            &[1, 2, 3, 4, 5, 6, 7, 8],
            &[1, 2, 3, 4, 5, 6, 7, 8],
        ),
        (
            Rgb,
            Eight,
            &[1, 2, 3, 4, 5, 6],
            &[1, 2, 3, 255, 4, 5, 6, 255],
        ),
        (
            Grayscale,
            Eight,
            &[9, 200],
            &[9, 9, 9, 255, 200, 200, 200, 255],
        ),
        (
            GrayscaleAlpha,
            Eight,
            &[9, 1, 200, 2],
            &[9, 9, 9, 1, 200, 200, 200, 2],
        ),
        (Indexed, Eight, &[0, 1], &[255, 0, 0, 255, 0, 255, 0, 128]),
        // Big-endian 16-bit samples keep their high byte.
        (
            Rgb,
            Sixteen,
            &[1, 99, 2, 99, 3, 99, 4, 99, 5, 99, 6, 99],
            &[1, 2, 3, 255, 4, 5, 6, 255],
        ),
    ];
    for (color, depth, data, rgba) in cases {
        let file = png_bytes(2, 1, color, depth, data);
        let image = decode("f=100", &file).unwrap();
        assert_eq!((image.width, image.height), (2, 1), "{color:?} {depth:?}");
        assert_eq!(image.rgba, rgba, "{color:?} {depth:?}");
        let image = decode("f=100,o=z", &compress_to_vec_zlib(&file, 6)).unwrap();
        assert_eq!(image.rgba, rgba, "{color:?} {depth:?} under zlib");
    }
}

#[test]
fn a_broken_or_oversized_png_is_refused() {
    let file = png_bytes(2, 2, png::ColorType::Rgba, png::BitDepth::Eight, &[7; 16]);
    assert_eq!(code(decode("f=100", &file[..file.len() - 20])), "EBADPNG");
    assert_eq!(code(decode("f=100", b"GIF89a")), "EBADPNG");
    assert_eq!(code(decode("f=100", &[])), "EBADPNG");
    assert_eq!(code(decode("f=100,o=z", b"not a zlib stream")), "EFBIG");
    let wide = png_bytes(
        4097,
        1,
        png::ColorType::Grayscale,
        png::BitDepth::Eight,
        &[0; 4097],
    );
    assert_eq!(code(decode("f=100", &wide)), "EINVAL");
}

#[test]
fn the_source_rectangle_stays_inside_the_image() {
    let rect = |control: &str| source_rect(&Command::parse(control.as_bytes()).unwrap(), (30, 20));
    let ok = |origin, size| Ok(SourceRect { origin, size });
    assert_eq!(rect(""), ok((0, 0), (30, 20)));
    assert_eq!(rect("x=5,y=6,w=7,h=8"), ok((5, 6), (7, 8)));
    // Too long a side stops at the edge; 0 reaches it.
    assert_eq!(rect("x=29,w=9,y=19"), ok((29, 19), (1, 1)));
    assert_eq!(rect("x=10,h=99"), ok((10, 0), (20, 20)));
    assert!(rect("x=30").is_err());
    assert!(rect("y=20").is_err());
}
