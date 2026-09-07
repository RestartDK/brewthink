extern crate std;

use embedded_graphics::{Drawable, pixelcolor::GrayColor};

use super::{DrawerSurface, FrameTarget, draw_reader_drawer};
use crate::{
    app::{App, AppEffect, AppInput, AppView, Direction},
    image::{PackedImage, READER_DEPTH, Size},
    ui::SELECTION_BACKGROUND,
};

fn reader_with_open_drawer() -> App {
    let mut app = App::new(1);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    assert!(matches!(
        app.input(AppInput::Confirm),
        AppEffect::LoadChapter { .. }
    ));
    assert_eq!(app.chapter_loaded(2, 3).unwrap(), AppEffect::Render);
    assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
    app
}

#[test]
fn drawer_preserves_rounded_corners_and_fades_the_page_behind_it() {
    let size = Size::new(480, 800).unwrap();
    let mut bytes = std::vec![0; READER_DEPTH.byte_len(size).unwrap()];
    let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
    DrawerSurface::new(304)
        .draw(&mut FrameTarget::new(&mut frame))
        .unwrap();
    assert_ne!(frame.luma(60, 100), 0);
    assert_eq!(frame.luma(61, 100), 0);
    assert_eq!(frame.luma(13, 306), 0);
    assert_eq!(frame.luma(240, 304), 0);
    for y in 312..334 {
        for x in 64..416 {
            assert_ne!(frame.luma(x, y), 0, "unexpected drag affordance at {x},{y}");
        }
    }
    assert_ne!(frame.luma(240, 340), 0);
    assert_eq!(frame.luma(240, 791), 0);
}

#[test]
fn every_drawer_item_uses_gray_selection_with_black_foreground_and_outline() {
    let mut app = reader_with_open_drawer();
    let tops = [400, 484, 562, 610, 658];
    let heights = [78, 72, 44, 44, 44];
    let foreground = [(44, 415), (40, 499), (35, 579), (34, 640), (43, 672)];
    let size = Size::new(480, 800).unwrap();

    for index in 0..tops.len() {
        let AppView::ReaderDrawer(drawer) = app.view() else {
            panic!("reader drawer expected")
        };
        let mut bytes = std::vec![0xff; READER_DEPTH.byte_len(size).unwrap()];
        let mut frame = PackedImage::new(size, READER_DEPTH, &mut bytes).unwrap();
        draw_reader_drawer(
            &mut FrameTarget::new(&mut frame),
            drawer,
            "A Small Book",
            "A named chapter",
            app.battery(),
        )
        .unwrap();

        assert_eq!(
            frame.luma(450, tops[index] + heights[index] / 2),
            SELECTION_BACKGROUND.luma(),
            "drawer row {index} lost its selection fill"
        );
        assert_eq!(
            frame.luma(240, tops[index]),
            0,
            "drawer row {index} lost its outline"
        );
        assert_eq!(
            frame.luma(foreground[index].0, foreground[index].1),
            0,
            "drawer row {index} lost its black icon"
        );

        app.input(AppInput::Move(Direction::Down));
    }
}
