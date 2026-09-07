use super::*;

#[test]
fn cover_can_be_dismissed_to_each_book_origin_before_loading_text() {
    for origin in [BookOrigin::Books, BookOrigin::Files] {
        let mut app = App::new(1);
        if origin == BookOrigin::Files {
            app.input(AppInput::Move(Direction::Down));
        }
        app.input(AppInput::Confirm);
        let parent = app.view();
        assert_eq!(app.input(AppInput::Confirm), AppEffect::Render);
        assert_eq!(
            app.view(),
            AppView::BookCover {
                book: BookId::new(0),
                origin
            }
        );
        assert_eq!(app.input(AppInput::Back), AppEffect::Render);
        assert_eq!(app.view(), parent);
        app.input(AppInput::Confirm);
        assert_eq!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                book: BookId::new(0),
                spine_index: 0,
                target: PageTarget::First
            }
        );
        app.chapter_loaded(3, 8).unwrap();
        app.input(AppInput::Move(Direction::Right));
        let progress = app.resume_point();
        app.input(AppInput::Back);
        assert!(matches!(
            app.input(AppInput::Confirm),
            AppEffect::LoadChapter {
                target: PageTarget::Index(1),
                ..
            }
        ));
        app.chapter_loaded(3, 8).unwrap();
        assert_eq!(app.resume_point(), progress);
    }
}

#[test]
fn sleeping_on_the_cover_resumes_the_start_of_the_book() {
    let mut app = App::new(1);
    app.input(AppInput::Confirm);
    app.input(AppInput::Confirm);
    app.input(AppInput::Power);
    assert!(matches!(
        app.resume_point(),
        ResumePoint::Reader {
            spine_index: 0,
            page_index: 0,
            ..
        }
    ));
    app.sleep_frame_ready().unwrap();
    assert!(matches!(
        app.wake(),
        AppEffect::LoadChapter {
            spine_index: 0,
            target: PageTarget::Index(0),
            ..
        }
    ));
    app.chapter_loaded(2, 6).unwrap();
    assert!(matches!(app.view(), AppView::Reader(_)));
}
