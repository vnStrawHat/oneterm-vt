//! Tests of the kitty graphics protocol, driven through a real `Terminal`.
//!
//! Ids: a terminal counts images and placements with one counter from 1, so a named image
//! takes an id when it is transmitted and each placement of it the next ones; an unnamed
//! image and a Sixel image are their own placement and take one id.

use std::time::Instant;

use miniz_oxide::deflate::compress_to_vec_zlib;

use super::super::terminal_tests::Session;
use super::*;
use crate::events::VtEvent;
use crate::graphics::kitty::kitty_tests::{base64, png_bytes};
use crate::snapshot::{SnapshotPlacement, SnapshotState};

/// One graphics command with its payload.
fn command(control: &str, payload: &[u8]) -> Vec<u8> {
    format!("\x1b_G{control};{}\x1b\\", base64(payload)).into_bytes()
}

/// `width * height` RGBA pixels, no two alike.
fn pixels(width: u32, height: u32) -> Vec<u8> {
    (0..width * height)
        .flat_map(|n| [n as u8, (n >> 8) as u8, 7, 255])
        .collect()
}

/// Sends one command and returns what the terminal answered.
fn send(s: &mut Session, control: &str, payload: &[u8]) -> String {
    s.feed(&command(control, payload));
    s.replies()
}

/// Sends a command that carries one RGB pixel: an image of one cell.
fn dot(s: &mut Session, control: &str) -> String {
    send(s, &format!("f=24,s=1,v=1,{control}"), &[1, 2, 3])
}

/// Moves the cursor to a 1-based row and column.
fn at(s: &mut Session, row: u16, col: u16) {
    s.feed(format!("\x1b[{row};{col}H").as_bytes());
}

/// The ids of the live placements, oldest first.
fn live(s: &Session) -> Vec<u64> {
    s.term.placements().iter().map(|p| p.id.0).collect()
}

/// The ids the last feed released: placements and images.
fn released(s: &Session) -> Vec<u64> {
    let ids = s.batch.iter().filter_map(|event| match event {
        VtEvent::GraphicReleased(id) => Some(id.0),
        _ => None,
    });
    ids.collect()
}

fn snapshot(s: &mut Session) -> Vec<SnapshotPlacement> {
    let mut state = SnapshotState::new();
    s.term.snapshot_update(&mut state, Instant::now());
    state.placements().to_vec()
}

fn ok(ids: &str) -> String {
    format!("\x1b_G{ids};OK\x1b\\")
}

#[test]
fn a_query_decodes_and_stores_nothing() {
    let mut s = Session::new(20, 5);
    assert_eq!(dot(&mut s, "a=q,i=31"), ok("i=31"));
    let file = png_bytes(
        1,
        1,
        png::ColorType::Rgba,
        png::BitDepth::Eight,
        &[1, 2, 3, 4],
    );
    assert_eq!(send(&mut s, "a=q,i=32,f=100", &file), ok("i=32"));
    assert!(live(&s).is_empty());
    assert!(s.term.take_graphics().is_empty());
    assert!(send(&mut s, "a=p,i=31", &[]).contains(";ENOENT:"));
}

#[test]
fn every_refusal_answers_with_its_code() {
    let cases: [(&str, &[u8], &str); 16] = [
        ("a=q,i=1,t=f", b"/tmp/image", "EINVAL"),
        ("a=q,i=1,t=s", b"name", "EINVAL"),
        ("a=q,i=1,I=2,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=q,i=1,f=8,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=q,i=1,o=x,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=q,i=1,v=1", &[0; 4], "EINVAL"),
        ("a=q,i=1,s=4097,v=1", &[0; 4], "EINVAL"),
        ("a=q,i=1,s=2,v=2", &[0; 3], "ENODATA"),
        ("a=q,i=1,o=z,s=1,v=1", b"not zlib", "ENODATA"),
        ("a=q,i=1,f=100", b"not a png", "EBADPNG"),
        ("a=p,i=1", &[], "ENOENT"),
        ("a=f,i=1", &[], "EINVAL"),
        ("a=T,i=1,U=1,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=T,i=1,P=3,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=T,i=1,x=1,s=1,v=1", &[0; 4], "EINVAL"),
        ("a=T,i=1,y=1,s=1,v=1", &[0; 4], "EINVAL"),
    ];
    for (control, payload, code) in cases {
        let mut s = Session::new(20, 5);
        let stats = s.feed(&command(control, payload));
        let reply = s.replies();
        assert!(
            reply.starts_with("\x1b_Gi=1")
                && reply.contains(&format!(";{code}:"))
                && reply.ends_with("\x1b\\"),
            "{control}: {reply:?}"
        );
        assert_eq!(stats.unhandled_sequences, 1, "{control}");
        assert!(live(&s).is_empty(), "{control}");
        assert!(s.term.take_graphics().is_empty(), "{control}");
    }
    let mut s = Session::new(20, 5);
    s.feed(b"\x1b_Ga=q,i=1,s=1,v=1,f=24;AA!A\x1b\\");
    assert!(s.replies().starts_with("\x1b_Gi=1;EINVAL:"));
}

#[test]
fn quiet_and_nameless_commands_answer_less() {
    let mut s = Session::new(20, 5);
    assert_eq!(dot(&mut s, "a=q,i=1,q=1"), "");
    assert!(send(&mut s, "a=q,i=1,q=1,s=9,v=9", &[0; 4]).contains(";ENODATA:"));
    assert_eq!(dot(&mut s, "a=q,i=1,q=2"), "");
    assert_eq!(send(&mut s, "a=q,i=1,q=2,s=9,v=9", &[0; 4]), "");
    // No id and no number: nothing to name the answer with, good or bad.
    assert_eq!(dot(&mut s, "a=T"), "");
    assert_eq!(send(&mut s, "a=T,s=9,v=9", &[0; 4]), "");
    assert_eq!(live(&s).len(), 1);
    // A malformed command is not answered either.
    let stats = s.feed(b"\x1b_Ga=q,i=1,s=x;AAAA\x1b\\");
    assert_eq!((s.replies().as_str(), stats.unhandled_sequences), ("", 1));
}

#[test]
fn transmit_and_place_puts_an_image_at_the_cursor() {
    let mut s = Session::new(20, 5);
    let rgba = pixels(25, 45);
    assert_eq!(send(&mut s, "a=T,i=5,p=2,s=25,v=45", &rgba), ok("i=5,p=2"));
    let placements = snapshot(&mut s);
    let [placement] = placements.as_slice() else {
        panic!("one placement: {placements:?}");
    };
    // 25 x 45 pixels on 10 x 20 cells.
    assert_eq!((placement.cols, placement.rows, placement.col), (3, 3, 0));
    assert_eq!(placement.row, s.term.screen().row_of_index(0));
    assert_eq!(placement.pixel_size, (25, 45));
    assert_eq!(
        (placement.source_origin, placement.source_size),
        ((0, 0), (25, 45))
    );
    assert_eq!(placement.display_size, (25, 45));
    assert_eq!((placement.pixel_offset, placement.z_index), ((0, 0), 0));
    // The image has an id of its own; the placement and its cells have another.
    let taken = s.term.take_graphics();
    assert_eq!(taken.len(), 1);
    assert_eq!(
        (taken[0].id, taken[0].width, taken[0].height),
        (placement.image, 25, 45)
    );
    assert_eq!(taken[0].rgba, rgba);
    assert_ne!(placement.image, placement.id);
    let screen = s.term.screen();
    let cell = screen.row(screen.row_of_index(2)).cell(2);
    let extras = s.term.interner().resolve_extras(cell.extras_id());
    assert_eq!(extras.graphic, Some(placement.id));
    // The cursor ends on the last row of the image, right of it.
    assert_eq!(s.cursor(), (2, 3));
}

#[test]
fn every_format_gives_the_same_pixels() {
    let rgba = pixels(4, 3);
    let rgb: Vec<u8> = rgba.chunks(4).flat_map(|p| [p[0], p[1], p[2]]).collect();
    let file = png_bytes(4, 3, png::ColorType::Rgba, png::BitDepth::Eight, &rgba);
    let cases: [(&str, Vec<u8>); 6] = [
        ("s=4,v=3", rgba.clone()),
        ("s=4,v=3,o=z", compress_to_vec_zlib(&rgba, 6)),
        ("f=24,s=4,v=3", rgb.clone()),
        ("f=24,s=4,v=3,o=z", compress_to_vec_zlib(&rgb, 6)),
        ("f=100", file.clone()),
        ("f=100,o=z", compress_to_vec_zlib(&file, 6)),
    ];
    for (control, payload) in cases {
        let mut s = Session::new(20, 5);
        assert_eq!(
            send(&mut s, &format!("a=T,i=1,{control}"), &payload),
            ok("i=1")
        );
        assert_eq!(s.term.take_graphics()[0].rgba, rgba, "{control}");
    }
}

#[test]
fn a_chunked_load_equals_an_unchunked_one() {
    let rgba = pixels(4, 4);
    let text = base64(&rgba);
    let mut bytes = format!("\x1b_Ga=T,i=3,s=4,v=4,m=1;{}\x1b\\", &text[..32]);
    bytes += &format!("\x1b_Gm=1;{}\x1b\\", &text[32..64]);
    bytes += &format!("\x1b_Gm=0;{}\x1b\\", &text[64..]);
    let mut s = Session::new(20, 5);
    s.feed(bytes.as_bytes());
    assert_eq!(s.replies(), ok("i=3"), "one answer, after the last chunk");
    assert_eq!(s.term.take_graphics()[0].rgba, rgba);

    // The same bytes, cut wherever the transport pleases.
    let mut s = Session::new(20, 5);
    let mut replies = String::new();
    for piece in bytes.as_bytes().chunks(7) {
        s.feed(piece);
        replies += &s.replies();
    }
    assert_eq!(replies, ok("i=3"));
    assert_eq!(s.term.take_graphics()[0].rgba, rgba);
}

#[test]
fn a_cancelled_chunk_drops_the_load() {
    let mut s = Session::new(20, 5);
    s.feed(b"\x1b_Ga=T,i=1,s=1,v=1,f=24,m=1;AAAA\x1b\\");
    s.feed(b"\x1b_Gm=0;AA\x18");
    assert_eq!(s.replies(), "");
    assert!(live(&s).is_empty());
    assert_eq!(dot(&mut s, "a=q,i=2"), ok("i=2"));
}

#[test]
fn a_stored_image_is_placed_as_often_as_asked_and_handed_out_once() {
    let mut s = Session::new(20, 5);
    assert_eq!(dot(&mut s, "a=t,i=1"), ok("i=1"));
    assert!(live(&s).is_empty(), "transmitting alone shows nothing");
    assert!(s.term.take_graphics().is_empty(), "and hands nothing out");
    assert_eq!(send(&mut s, "a=p,i=1", &[]), ok("i=1"));
    assert_eq!(send(&mut s, "a=p,i=1", &[]), ok("i=1"));
    assert_eq!(live(&s), [2, 3]);
    let taken = s.term.take_graphics();
    assert_eq!(taken.len(), 1, "both placements show the same pixels");
    assert_eq!(
        (taken[0].id.0, &taken[0].rgba[..]),
        (1, &[1, 2, 3, 255][..])
    );
    assert!(s.term.placements().iter().all(|p| p.image == taken[0].id));
    assert_eq!(s.term.state.graphics.live_bytes, 4, "counted once");
    assert_eq!(s.cursor(), (0, 2));
    // A third placement needs no pixels at all.
    send(&mut s, "a=p,i=1", &[]);
    assert!(s.term.take_graphics().is_empty());
}

#[test]
fn an_image_is_released_after_its_last_placement_unless_it_is_stored() {
    // An unnamed image is its own placement: one id, one release.
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T");
    send(&mut s, "a=d", &[]);
    assert_eq!(released(&s), [1]);
    // A stored image outlives its placements, until the program deletes it too.
    dot(&mut s, "a=T,i=1");
    send(&mut s, "a=p,i=1", &[]);
    assert_eq!(live(&s), [3, 4]);
    send(&mut s, "a=d,d=i,i=1,p=0", &[]);
    assert_eq!(released(&s), [3, 4]);
    assert_eq!(s.term.state.graphics.live_bytes, 0);
    send(&mut s, "a=d,d=I,i=1", &[]);
    assert_eq!(released(&s), [2], "the pixels the embedder holds");
    // Placements first, then the image, when one command ends both.
    dot(&mut s, "a=T,i=1");
    send(&mut s, "a=d,d=I,i=1", &[]);
    assert_eq!(released(&s), [6, 5]);
    // An image that was never placed was never handed out: no release.
    dot(&mut s, "a=t,i=1");
    send(&mut s, "a=d,d=I,i=1", &[]);
    assert!(released(&s).is_empty());
}

#[test]
fn a_placement_id_names_one_placement_that_moves() {
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=t,i=1");
    assert_eq!(send(&mut s, "a=p,i=1,p=9", &[]), ok("i=1,p=9"));
    at(&mut s, 3, 5);
    assert_eq!(send(&mut s, "a=p,i=1,p=9", &[]), ok("i=1,p=9"));
    assert_eq!(released(&s), [2]);
    assert_eq!(live(&s), [3]);
    let placements = snapshot(&mut s);
    assert_eq!(
        (placements[0].row, placements[0].col),
        (s.term.screen().row_of_index(2), 4)
    );
    // Another placement id, and none at all, add placements.
    send(&mut s, "a=p,i=1,p=8", &[]);
    send(&mut s, "a=p,i=1", &[]);
    assert_eq!(live(&s), [3, 4, 5]);
}

#[test]
fn transmitting_an_id_again_ends_its_placements() {
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T,i=1");
    dot(&mut s, "a=T,i=2");
    send(&mut s, "a=p,i=1", &[]);
    assert_eq!(live(&s), [2, 4, 5]);
    s.term.take_graphics();
    send(&mut s, "a=t,i=1,f=24,s=1,v=1", &[9, 9, 9]);
    assert_eq!(
        released(&s),
        [2, 5, 1],
        "both placements, then the old pixels"
    );
    assert_eq!(live(&s), [4]);
    send(&mut s, "a=p,i=1", &[]);
    assert_eq!(s.term.take_graphics()[0].rgba, [9, 9, 9, 255]);
}

#[test]
fn a_placement_shows_its_source_rectangle() {
    let mut s = Session::new(20, 5);
    let rgba = pixels(4, 4);
    send(&mut s, "a=t,i=1,s=4,v=4", &rgba);
    send(&mut s, "a=p,i=1,x=1,y=2,w=2,h=1", &[]);
    // Too wide a rectangle stops at the edge.
    send(&mut s, "a=p,i=1,x=3,w=7,h=1", &[]);
    let placements = snapshot(&mut s);
    assert_eq!(placements[0].pixel_size, (4, 4));
    assert_eq!(
        (placements[0].source_origin, placements[0].source_size),
        ((1, 2), (2, 1))
    );
    assert_eq!(placements[0].display_size, (2, 1));
    assert_eq!(
        (placements[1].source_origin, placements[1].source_size),
        ((3, 0), (1, 1))
    );
    // The whole image is handed out once; the renderer cuts.
    let taken = s.term.take_graphics();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].rgba, rgba);
    assert!(send(&mut s, "a=p,i=1,x=4", &[]).contains(";EINVAL:"));
    // An unnamed image has a source rectangle too.
    send(&mut s, "a=T,s=4,v=4,y=3", &rgba);
    let placement = snapshot(&mut s)[2];
    assert_eq!(
        (placement.source_origin, placement.source_size),
        ((0, 3), (4, 1))
    );
    assert_eq!(placement.image, placement.id);
    assert_eq!(s.term.take_graphics()[0].rgba, rgba);
}

#[test]
fn columns_and_rows_decide_the_display_size() {
    let cell = (10, 20);
    let shape = |control: &str| {
        let geometry = geometry(&Command::parse(control.as_bytes()).unwrap(), (25, 45), cell);
        (
            geometry.display_size,
            geometry.cols,
            geometry.rows,
            geometry.pixel_offset,
        )
    };
    // Neither: one image pixel per pixel, as many cells as that takes.
    assert_eq!(shape(""), ((25, 45), 3, 3, (0, 0)));
    assert_eq!(shape("X=7,Y=16"), ((25, 45), 4, 4, (7, 16)));
    // Both: stretched over the cells, from the offset on.
    assert_eq!(shape("c=4,r=2"), ((40, 40), 4, 2, (0, 0)));
    assert_eq!(shape("c=4,r=2,X=3,Y=5"), ((37, 35), 4, 2, (3, 5)));
    // One: the other side keeps the aspect ratio.
    assert_eq!(shape("c=5"), ((50, 90), 5, 5, (0, 0)));
    assert_eq!(shape("r=3"), ((33, 60), 4, 3, (0, 0)));
    // An offset stays inside the cell; what `c` and `r` ask for is capped.
    assert_eq!(shape("X=99,Y=99").3, (9, 19));
    assert_eq!(shape("r=4294967295").2, MAX_ROWS);
    assert_eq!(shape("c=4294967295").1, 65_535);
}

#[test]
fn a_placement_carries_its_offset_size_and_layer() {
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T,i=1,X=3,Y=4,c=2,r=1,z=-1");
    let placement = snapshot(&mut s)[0];
    assert_eq!((placement.cols, placement.rows), (2, 1));
    assert_eq!(placement.pixel_size, (1, 1));
    assert_eq!(placement.pixel_offset, (3, 4));
    assert_eq!(placement.display_size, (17, 16));
    assert_eq!(placement.z_index, -1);
    assert_eq!(s.term.placements()[0].display_size, (17, 16));
}

#[test]
fn the_rows_asked_for_are_bounded_by_the_screen_and_the_tallest_image() {
    // 4096 pixels on 20-pixel cells are 205 rows; a 5-row screen asks for no more.
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T,r=4000,C=1");
    assert_eq!(snapshot(&mut s)[0].rows, 205);
    // One-pixel cells: 4096 rows.
    let mut s = Session::new(20, 5);
    s.term.set_cell_pixels(1, 1);
    dot(&mut s, "a=T,r=60000,C=1");
    assert_eq!(snapshot(&mut s)[0].rows, 4096);
}

#[test]
fn columns_are_clipped_at_the_right_edge() {
    let mut s = Session::new(5, 4);
    at(&mut s, 1, 4);
    dot(&mut s, "a=T,i=1,c=4,r=1");
    let placement = snapshot(&mut s)[0];
    assert_eq!((placement.col, placement.cols), (3, 2));
    assert_eq!(
        placement.display_size,
        (40, 20),
        "the renderer clips the pixels"
    );
    assert_eq!(s.cursor(), (1, 0), "past the last column: the next line");
}

#[test]
fn the_cursor_ends_right_of_the_last_image_row() {
    let mut s = Session::new(10, 4);
    at(&mut s, 1, 8);
    send(&mut s, "a=T,s=20,v=40", &pixels(20, 40));
    assert_eq!(s.cursor(), (1, 9));
    // At the right edge it wraps to the next line.
    let mut s = Session::new(10, 4);
    at(&mut s, 1, 8);
    send(&mut s, "a=T,s=30,v=20", &pixels(30, 20));
    assert_eq!(s.cursor(), (1, 0));
}

#[test]
fn an_image_at_the_bottom_scrolls_and_stays_with_its_rows() {
    let mut s = Session::new(10, 4);
    at(&mut s, 4, 2);
    send(&mut s, "a=T,i=1,s=10,v=60", &pixels(10, 60));
    assert_eq!(s.cursor(), (3, 2));
    let placement = snapshot(&mut s)[0];
    assert_eq!((placement.rows, placement.col), (3, 1));
    assert_eq!(placement.row, s.term.screen().row_of_index(1));
    // More output: the image goes into history with its rows and stays live.
    s.feed(b"\r\n\r\n\r\n\r\n\r\n");
    assert_eq!(live(&s), [2]);
    assert!(snapshot(&mut s)[0].row < s.term.screen().row_of_index(0));
}

#[test]
fn the_cursor_stays_when_asked() {
    let mut s = Session::new(10, 4);
    at(&mut s, 2, 3);
    send(&mut s, "a=T,i=1,C=1,s=30,v=60", &pixels(30, 60));
    assert_eq!(s.cursor(), (1, 2));
    let placement = snapshot(&mut s)[0];
    assert_eq!((placement.cols, placement.rows), (3, 3));
    assert_eq!(placement.row, s.term.screen().row_of_index(1));
    // The rows below the cursor row carry the image too.
    let screen = s.term.screen();
    let cell = screen.row(screen.row_of_index(3)).cell(4);
    let extras = s.term.interner().resolve_extras(cell.extras_id());
    assert_eq!(extras.graphic, Some(placement.id));
}

/// Four one-cell placements on a 20 x 10 screen (image 1 has id 1, image 2 has id 4):
/// 2 = image 1, placement 1, layer 0, at row 1 column 1;
/// 3 = image 1, placement 2, layer 5, at row 3 column 4;
/// 5 = image 2, layer 5, at row 5 column 6;
/// 6 = an unnamed image, layer 0, at row 3 column 8.
fn scene() -> Session {
    let mut s = Session::new(20, 10);
    at(&mut s, 1, 1);
    dot(&mut s, "a=T,i=1,p=1,C=1");
    at(&mut s, 3, 4);
    send(&mut s, "a=p,i=1,p=2,z=5,C=1", &[]);
    at(&mut s, 5, 6);
    dot(&mut s, "a=T,i=2,z=5,C=1");
    at(&mut s, 3, 8);
    dot(&mut s, "a=T,C=1");
    assert_eq!(live(&s), [2, 3, 5, 6]);
    at(&mut s, 3, 4);
    s
}

/// Whether the store of the active screen still holds image `id`.
fn stored(s: &mut Session, id: u32) -> bool {
    send(s, &format!("a=p,i={id},C=1,q=1"), &[]).is_empty()
}

#[test]
fn a_delete_releases_the_placements_it_names() {
    let cases: [(&str, &[u64]); 19] = [
        ("a=d", &[]),
        ("a=d,d=a", &[]),
        ("a=d,d=i,i=1", &[5, 6]),
        ("a=d,d=i,i=1,p=2", &[2, 5, 6]),
        ("a=d,d=i,i=9", &[2, 3, 5, 6]),
        ("a=d,d=i", &[2, 3, 5, 6]),
        ("a=d,d=c", &[2, 5, 6]),
        ("a=d,d=p,x=4,y=3", &[2, 5, 6]),
        ("a=d,d=p,x=0,y=3", &[2, 3, 5, 6]),
        ("a=d,d=p,x=4,y=11", &[2, 3, 5, 6]),
        ("a=d,d=q,x=4,y=3,z=0", &[2, 3, 5, 6]),
        ("a=d,d=q,x=4,y=3,z=5", &[2, 5, 6]),
        ("a=d,d=x,x=1", &[3, 5, 6]),
        ("a=d,d=y,y=3", &[2, 5]),
        ("a=d,d=z,z=5", &[2, 6]),
        ("a=d,d=r,x=2,y=5", &[2, 3, 6]),
        ("a=d,d=r,x=1,y=2", &[6]),
        ("a=d,d=f", &[2, 3, 5, 6]),
        ("a=d,d=n,I=1", &[2, 3, 5, 6]),
    ];
    for (control, left) in cases {
        let mut s = scene();
        assert_eq!(send(&mut s, control, &[]), "", "a delete is never answered");
        assert_eq!(live(&s), left, "{control}");
        // A lower-case delete keeps the stored images.
        assert!(stored(&mut s, 1) && stored(&mut s, 2), "{control}");
    }
}

#[test]
fn an_upper_case_delete_forgets_images_left_without_a_placement() {
    // (command, placements left, image 1 stored, image 2 stored)
    let cases: [(&str, &[u64], bool, bool); 8] = [
        ("a=d,d=A", &[], false, false),
        ("a=d,d=I,i=1", &[5, 6], false, true),
        ("a=d,d=I,i=1,p=2", &[2, 5, 6], true, true),
        ("a=d,d=C", &[2, 5, 6], true, true),
        ("a=d,d=Z,z=5", &[2, 6], true, false),
        ("a=d,d=R,x=1,y=1", &[5, 6], false, true),
        ("a=d,d=Y,y=5", &[2, 3, 6], true, false),
        ("a=d,d=X,x=8", &[2, 3, 5], true, true),
    ];
    for (control, left, first, second) in cases {
        let mut s = scene();
        send(&mut s, control, &[]);
        assert_eq!(live(&s), left, "{control}");
        assert_eq!(
            (stored(&mut s, 1), stored(&mut s, 2)),
            (first, second),
            "{control}"
        );
    }
    // An image that was never placed is forgotten by its id alone.
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=t,i=6");
    send(&mut s, "a=d,d=I,i=6", &[]);
    assert!(!stored(&mut s, 6));
}

#[test]
fn a_number_names_an_image_the_engine_gave_an_id() {
    let mut s = Session::new(20, 5);
    let id = 1u32 << 31;
    assert_eq!(dot(&mut s, "a=T,I=7"), ok(&format!("i={id},I=7")));
    assert_eq!(
        send(&mut s, "a=p,I=7,p=3", &[]),
        ok(&format!("i={id},I=7,p=3"))
    );
    assert_eq!(live(&s), [2, 3]);
    send(&mut s, "a=d,d=n,I=7,p=3", &[]);
    assert_eq!(live(&s), [2]);
    send(&mut s, "a=d,d=n,I=7", &[]);
    assert!(live(&s).is_empty());
    assert_eq!(send(&mut s, "a=p,I=7", &[]), ok(&format!("i={id},I=7")));
    send(&mut s, "a=d,d=N,I=7", &[]);
    assert!(send(&mut s, "a=p,I=7", &[]).starts_with("\x1b_GI=7;ENOENT:"));
}

#[test]
fn a_delete_leaves_sixel_images_and_history_alone() {
    let mut s = Session::new(20, 4);
    s.feed(b"\x1bPq\"1;1;1;1#0;2;100;0;0#0@\x1b\\");
    at(&mut s, 2, 5);
    dot(&mut s, "a=T,i=1");
    assert_eq!(live(&s), [1, 3]);
    send(&mut s, "a=d,d=A", &[]);
    assert_eq!(live(&s), [1], "the Sixel image stays");
    // A placement whose rows are all in history is not on the screen.
    dot(&mut s, "a=T,i=2");
    s.feed(b"\r\n\r\n\r\n\r\n\r\n\r\n");
    send(&mut s, "a=d,d=A", &[]);
    assert_eq!(live(&s), [1, 5]);
}

#[test]
fn each_screen_has_its_own_images() {
    let mut s = Session::new(20, 5);
    send(&mut s, "a=t,i=1,f=24,s=1,v=1", &[1, 1, 1]);
    s.feed(b"\x1b[?1049h");
    assert!(send(&mut s, "a=p,i=1", &[]).contains(";ENOENT:"));
    send(&mut s, "a=T,i=1,f=24,s=1,v=1", &[2, 2, 2]);
    send(&mut s, "a=d,d=A", &[]);
    s.feed(b"\x1b[?1049l");
    s.term.take_graphics();
    assert_eq!(send(&mut s, "a=p,i=1", &[]), ok("i=1"));
    assert_eq!(s.term.take_graphics()[0].rgba, [1, 1, 1, 255]);
}

#[test]
fn a_full_reset_forgets_the_stored_images() {
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T,i=1");
    dot(&mut s, "a=T,i=2");
    send(&mut s, "a=d,d=i,i=2", &[]);
    s.feed(b"\x1b_Ga=t,i=3,s=1,v=1,f=24,m=1;AAAA\x1b\\");
    s.feed(b"\x1bc");
    // Image 2 (id 3) had no placement left; image 1 (id 1) goes after its placement (id 2).
    assert_eq!(released(&s), [3, 2, 1]);
    assert!(!stored(&mut s, 1) && !stored(&mut s, 2));
    // The load that was open is gone: this is a command of its own.
    assert_eq!(dot(&mut s, "a=q,i=3"), ok("i=3"));
}

#[test]
fn a_covering_image_releases_only_the_layers_it_is_not_under() {
    let mut s = Session::new(20, 5);
    dot(&mut s, "a=T,z=5,C=1");
    dot(&mut s, "a=T,C=1");
    assert_eq!(live(&s), [1, 2], "the image above stays");
    dot(&mut s, "a=T,z=5,C=1");
    assert_eq!(live(&s), [3]);
}

#[test]
fn pixels_dropped_from_the_queue_are_handed_out_by_the_next_placement() {
    // 32 MiB, 64 MiB and 64 MiB of pixels, a cell each: the queue holds 128 MiB, and the
    // store keeps the two named ones (96 MiB).
    let payload = compress_to_vec_zlib(&vec![0; 4096 * 4096 * 4], 1);
    let mut s = Session::new(20, 5);
    s.term.set_cell_pixels(4096, 4096);
    let controls = [
        "a=T,i=1,o=z,s=4096,v=2048",
        "a=T,i=2,o=z,s=4096,v=4096",
        "a=T,o=z,s=4096,v=4096",
    ];
    for (index, control) in controls.into_iter().enumerate() {
        let stats = s.feed(&command(control, &payload));
        assert_eq!(stats.graphics_dropped, u32::from(index == 2), "{control}");
    }
    let taken = |s: &mut Session| -> Vec<u64> {
        let images = s.term.take_graphics();
        images.iter().map(|data| data.id.0).collect()
    };
    // Image 1 (id 1) left the queue, so its placement (id 2) went; the store keeps it.
    assert_eq!(released(&s), [2]);
    assert_eq!(live(&s), [4, 5]);
    assert_eq!(taken(&mut s), [3, 5]);
    assert_eq!(send(&mut s, "a=p,i=1", &[]), ok("i=1"));
    assert_eq!(taken(&mut s), [1]);
}

#[test]
fn text_and_other_strings_pass_by() {
    let mut s = Session::new(20, 5);
    let stats = s.feed(b"a\x1b_not graphics\x1b\\b\x1b^Ga=q,i=1\x1b\\c");
    assert_eq!(stats.unhandled_sequences, 2);
    assert_eq!((s.row(0).trim_end(), s.replies().as_str()), ("abc", ""));
}

mod hostile {
    use std::collections::HashSet;

    use proptest::prelude::*;

    use super::*;

    /// One `key=value` pair: mostly small values, so ids, cells and sizes collide often.
    fn pair() -> impl Strategy<Value = String> {
        let key = prop::sample::select("atodzqfmsviIpxywhXYcrCUP".chars().collect::<Vec<char>>());
        let value = prop_oneof![
            (0u32..6).prop_map(|n| n.to_string()),
            prop::sample::select(vec!["24", "32", "100", "4097", "-1", "4294967295", ""])
                .prop_map(str::to_owned),
            prop::sample::select("tTqpdzaAiIcCnNrRxXyY".chars().collect::<Vec<char>>())
                .prop_map(|letter| letter.to_string()),
        ];
        (key, value).prop_map(|(key, value)| format!("{key}={value}"))
    }

    /// A payload: pixels, a PNG, a zlib stream, or bytes that are none of them.
    fn payload() -> impl Strategy<Value = Vec<u8>> {
        prop_oneof![
            prop::collection::vec(any::<u8>(), 0..48),
            (1u32..4, 1u32..4).prop_map(|(width, height)| {
                let data = pixels(width, height);
                png_bytes(
                    width,
                    height,
                    png::ColorType::Rgba,
                    png::BitDepth::Eight,
                    &data,
                )
            }),
            prop::collection::vec(any::<u8>(), 0..48)
                .prop_map(|data| compress_to_vec_zlib(&data, 1)),
        ]
    }

    /// The bytes of one step: a graphics command, or something that moves the grid under it.
    fn step() -> impl Strategy<Value = Vec<u8>> {
        let graphics = (
            prop::collection::vec(pair(), 0..6),
            payload(),
            any::<bool>(),
        )
            .prop_map(|(pairs, payload, cancelled)| {
                let end = if cancelled { "\x18" } else { "\x1b\\" };
                format!("\x1b_G{};{}{end}", pairs.join(","), base64(&payload)).into_bytes()
            });
        let other = prop::sample::select(vec![
            &b"\r\n"[..],
            b"text",
            b"\x1b[2J",
            b"\x1b[H",
            b"\x1b[3;4H",
            b"\x1b[?1049h",
            b"\x1b[?1049l",
            b"\x1bc",
            b"\x1b[2L",
            b"\x1bPq\"1;1;1;1#0@\x1b\\",
        ])
        .prop_map(<[u8]>::to_vec);
        prop_oneof![3 => graphics, 1 => other]
    }

    proptest! {
        #[test]
        fn any_stream_of_commands_keeps_the_books(steps in prop::collection::vec(step(), 1..60)) {
            let mut s = Session::new(12, 6);
            for (index, bytes) in steps.iter().enumerate() {
                s.feed(bytes);
                if index % 3 == 0 {
                    s.term.take_graphics();
                }
            }
            let graphics = &s.term.state.graphics;
            // Every image is counted once, however many placements show it.
            let mut images = HashSet::new();
            let mut bytes = 0;
            for placement in graphics.placements.iter() {
                if images.insert(placement.image) {
                    let (width, height) = placement.pixel_size;
                    bytes += width as usize * height as usize * 4;
                }
            }
            prop_assert_eq!(graphics.live_bytes, bytes);
            // Every kitty entry names a live placement, once.
            let live: HashSet<u64> = graphics.placements.iter().map(|p| p.id.0).collect();
            let mut named = HashSet::new();
            for entry in &graphics.kitty.refs {
                prop_assert!(live.contains(&entry.graphic.0));
                prop_assert!(named.insert(entry.graphic.0));
            }
            // A source rectangle lies inside its image.
            for placement in graphics.placements.iter() {
                let (x, y) = placement.source_origin;
                let (width, height) = placement.source_size;
                prop_assert!(width >= 1 && x + width <= placement.pixel_size.0);
                prop_assert!(height >= 1 && y + height <= placement.pixel_size.1);
            }
        }
    }
}
