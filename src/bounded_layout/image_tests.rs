extern crate std;
use super::*;
use crate::{
    image::{PackedImage, READER_DEPTH, Size},
    power::BatteryStatus,
    reader::{
        BODY_TOP, FRAME_HEIGHT, FRAME_WIDTH, ReaderLine, ReaderView, render_inline_image,
        render_reader_with_images,
    },
};
use std::{vec, vec::Vec};

fn resource(path: &str, width: usize, height: usize) -> ImageResource {
    ImageResource {
        path: FixedString::try_from_str(path).unwrap(),
        size: Size::new(width, height).unwrap(),
        crc32: 1,
        bytes: 100,
    }
}

#[test]
fn text_and_pixels_keep_their_reserved_positions_on_the_same_page() {
    let mut page = BoundedPage::new();
    layout_xhtml_page_with_images_into(
        b"<body><p>Above</p><img src='diagram.png' alt='A diagram'/><p>Below</p></body>",
        0,
        ReaderPreferences::default(),
        &mut page,
        |path| Some(resource(path, 77, 40)),
    )
    .unwrap();
    assert_eq!(page.page_count(), 1);
    let image = page.images().next().unwrap();
    assert_eq!(image.spec().size(), Size::new(80, 40).unwrap());
    let above = page.lines().find(|line| line.text() == "Above").unwrap();
    let below = page.lines().find(|line| line.text() == "Below").unwrap();
    assert!(above.top() < image.top());
    assert!(below.top() >= image.top() + image.spec().size().height());
    assert_eq!(image.alt(), "A diagram");
    let lines: Vec<_> = page
        .lines()
        .map(|line| ReaderLine::new(line.text(), line.style()).with_top(line.top() as u16))
        .collect();
    let image_bytes = vec![0; image.spec().byte_len()];
    let bitmap =
        crate::image::PackedBitmap::new(image.spec().size(), READER_DEPTH, &image_bytes).unwrap();
    let mut pixels = vec![255; FRAME_WIDTH * FRAME_HEIGHT / 4];
    let mut target = PackedImage::new(
        Size::new(FRAME_WIDTH, FRAME_HEIGHT).unwrap(),
        READER_DEPTH,
        &mut pixels,
    )
    .unwrap();
    let view = ReaderView::new(
        "Book",
        "Chapter",
        &lines,
        ReaderPreferences::default(),
        BatteryStatus::unknown(),
    );
    render_reader_with_images(view, &mut target, |target, offset| {
        assert!(render_inline_image(image, Some(bitmap), target, offset));
    })
    .unwrap();
    let left = (FRAME_WIDTH - image.spec().size().width()) / 2;
    let top = BODY_TOP + image.top();
    for y in top..top + image.spec().size().height() {
        for x in left..left + image.spec().size().width() {
            assert_eq!(target.bitmap().level(x, y), 0);
        }
    }
    assert_eq!(target.bitmap().level(left - 1, top), 3);
    assert_eq!(target.bitmap().level(left, top - 1), 3);
    assert_eq!(
        target
            .bitmap()
            .level(left, top + image.spec().size().height()),
        3
    );
    assert!((BODY_TOP..top).any(|y| (18..100).any(|x| target.bitmap().level(x, y) == 0)));
    assert!(
        (BODY_TOP + below.top()..BODY_TOP + below.top() + 16)
            .any(|y| (18..100).any(|x| target.bitmap().level(x, y) == 0))
    );
}

#[test]
fn full_height_figures_move_as_a_unit_and_navigation_is_repeatable() {
    let xhtml = b"<body><p>Before</p><img src='tall.png'/><p>After</p></body>";
    let mut first = BoundedPage::new();
    let mut page = BoundedPage::new();
    layout_xhtml_page_with_images_into(
        xhtml,
        0,
        ReaderPreferences::default(),
        &mut first,
        |path| Some(resource(path, 100, 4000)),
    )
    .unwrap();
    assert_eq!(first.page_count(), 3);
    assert_eq!(first.images().count(), 0);
    for index in [1, 2, 0, 1] {
        layout_xhtml_page_with_images_into(
            xhtml,
            index,
            ReaderPreferences::default(),
            &mut page,
            |path| Some(resource(path, 100, 4000)),
        )
        .unwrap();
        assert_eq!(page.page_count(), 3);
        assert_eq!(page.images().count(), usize::from(index == 1));
        for image in page.images() {
            assert_eq!(image.top(), 0);
            assert_eq!(image.spec().size().height(), PAGE_HEIGHT);
        }
    }
}

#[test]
fn unsupported_images_keep_alt_text_and_svg_image_links_use_the_same_resolver() {
    let mut page = BoundedPage::new();
    let xhtml = b"<body><img src='missing.gif' alt='Caption for missing image'/><svg><image xlink:href='picture.png'/></svg></body>";
    layout_xhtml_page_with_images_into(xhtml, 0, ReaderPreferences::default(), &mut page, |path| {
        (path == "picture.png").then(|| resource(path, 80, 80))
    })
    .unwrap();
    let text = page
        .lines()
        .map(|line| line.text())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("Caption for missing image"), "{text:?}");
    assert_eq!(page.images().count(), 1);
    assert_eq!(page.images().next().unwrap().path(), "picture.png");
}
