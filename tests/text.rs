use std::iter::zip;

use buoyant::{
    environment::DefaultEnvironment,
    event::{EventContext, TouchResult},
    font::CharacterBufferFont,
    primitives::{Dimensions, Point, Size},
    render::Render,
    render_target::FixedTextBuffer,
    view::prelude::*,
};
mod common;
use common::make_render_tree;

#[test]
fn test_render_wrapping_leading() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 5>::default();
    let text = Text::new("This is a lengthy text here", &font).foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "This  ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), "is a  ");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "length");
    assert_eq!(buffer.text[3].iter().collect::<String>(), "y text");
    assert_eq!(buffer.text[4].iter().collect::<String>(), "here  ");
}

#[test]
fn test_render_wrapping_center_even() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 5>::default();
    let text = Text::new("This is a lengthy text here", &font)
        .multiline_text_alignment(HorizontalTextAlignment::Center)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), " This ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), " is a ");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "length");
    assert_eq!(buffer.text[3].iter().collect::<String>(), "y text");
    assert_eq!(buffer.text[4].iter().collect::<String>(), " here ");
}

#[test]
fn test_render_wrapping_center_odd() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 5>::default();
    let text = Text::new("This is a lengthy text 12345", &font)
        .multiline_text_alignment(HorizontalTextAlignment::Center)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), " This ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), " is a ");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "length");
    assert_eq!(buffer.text[3].iter().collect::<String>(), "y text");
    assert_eq!(buffer.text[4].iter().collect::<String>(), "12345 ");
}

#[test]
fn test_render_wrapping_trailing() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 5>::default();
    let text = Text::new("This is a lengthy text here", &font)
        .multiline_text_alignment(HorizontalTextAlignment::Trailing)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "  This");
    assert_eq!(buffer.text[1].iter().collect::<String>(), "  is a");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "length");
    assert_eq!(buffer.text[3].iter().collect::<String>(), "y text");
    assert_eq!(buffer.text[4].iter().collect::<String>(), "  here");
}

#[test]
fn test_clipped_text_is_centered_correctly() {
    let font = CharacterBufferFont {};
    let view = Text::new(
        "Several lines\n of text\nshould be correctly spaced when cut off",
        &font,
    )
    .multiline_text_alignment(HorizontalTextAlignment::Center)
    .foreground_color(' ');

    let env = DefaultEnvironment::non_animated();
    let mut buffer = FixedTextBuffer::<40, 2>::default();

    view.build_state(&mut ());
    let layout = view.layout(&buffer.size().into(), &env, &mut (), &mut ());

    assert_eq!(layout.resolved_size, Dimensions::new(13, 2));

    let tree = view.render_tree(&layout.sublayouts, Point::zero(), &env, &mut (), &mut ());
    tree.render(&mut buffer, &' ');

    let lines = [
        "Several lines                           ",
        "   of text                              ",
    ];
    zip(lines.iter(), buffer.text.iter()).for_each(|(expected, actual)| {
        assert_eq!(actual.iter().collect::<String>(), *expected);
    });
}

#[test]
fn test_clipped_text_trails_correctly() {
    let font = CharacterBufferFont {};
    let view = Text::new(
        "Several lines\n of text\nshould be correctly spaced when cut off",
        &font,
    )
    .multiline_text_alignment(HorizontalTextAlignment::Trailing)
    .frame()
    .with_height(2) // constrain to 2 pts tall
    .foreground_color(' ');

    let env = DefaultEnvironment::non_animated();
    let mut buffer = FixedTextBuffer::<40, 3>::default();

    view.build_state(&mut ());
    let layout = view.layout(&buffer.size().into(), &env, &mut (), &mut ());

    assert_eq!(layout.resolved_size, Dimensions::new(13, 2));

    let tree = view.render_tree(&layout.sublayouts, Point::zero(), &env, &mut (), &mut ());
    tree.render(&mut buffer, &' ');

    let lines = [
        "Several lines                           ",
        "      of text                           ",
        "                                        ",
    ];
    zip(lines.iter(), buffer.text.iter()).for_each(|(expected, actual)| {
        assert_eq!(actual.iter().collect::<String>(), *expected);
    });
}

#[test]
fn format_args_fits() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<12, 1>::default();
    let view = |a: u8, b: u8| {
        Text::new_fmt::<12>(format_args!("{} + {} = {}", a, b, a + b), &font)
            .multiline_text_alignment(HorizontalTextAlignment::Leading)
            .foreground_color(' ')
    };
    for (a, b) in [(1, 1), (2, 3), (45, 6)] {
        buffer.clear();
        make_render_tree(&view(a, b), buffer.size(), &mut ()).render(&mut buffer, &' ');
        assert_eq!(
            buffer.text[0].iter().collect::<String>(),
            format!("{} + {} = {}    ", a, b, a + b).get(0..12).unwrap()
        );
    }
}

#[test]
fn undersized_format_args() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<15, 8>::default();
    let view = |a: u32, b: u32| {
        VStack::new((
            Text::new_fmt::<3>(format_args!("{}+{}={}", a, b, a + b), &font),
            Text::new_fmt::<5>(format_args!("{}+{}={}", a, b, a + b), &font),
            Text::new_fmt::<9>(format_args!("{}+{}={}", a, b, a + b), &font),
            Text::new_fmt::<11>(format_args!("{}+{}={}", a, b, a + b), &font),
        ))
        .with_alignment(HorizontalAlignment::Leading)
        .foreground_color(' ')
    };
    make_render_tree(&view(123, 456), buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "123            ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), "123+           ");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "123+456=       ");
    assert_eq!(buffer.text[3].iter().collect::<String>(), "123+456=579    ");
}

#[test]
fn multibyte_char_wrapping_does_not_panic() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<15, 8>::default();

    let view = HStack::new((
        buoyant::view::Text::new("Temp:", &font),
        buoyant::view::Text::new("252°C", &font),
    ))
    .with_spacing(5)
    .with_alignment(buoyant::layout::VerticalAlignment::Top)
    .foreground_color(' ');

    for x in 0..buffer.size().width {
        for y in 0..buffer.size().height {
            buffer.clear();
            let layout_size = Size::new(x, y);
            let tree = make_render_tree(&view, layout_size, &mut ());
            tree.render(&mut buffer, &' ');
        }
    }
}

#[allow(clippy::let_unit_value)]
#[test]
fn text_leaves_touches_to_the_views_around_it() {
    let font = CharacterBufferFont;
    let view = Text::new("abc", &font);
    let mut state = view.build_state(&mut ());
    let layout = view.layout(
        &Size::new(10, 10).into(),
        &DefaultEnvironment::default(),
        &mut (),
        &mut state,
    );
    let mut tree = view.render_tree(
        &layout.sublayouts,
        Point::zero(),
        &DefaultEnvironment::default(),
        &mut (),
        &mut state,
    );
    let ctx = EventContext::new(core::time::Duration::ZERO);

    // "abc" occupies (0,0)-(3,1); a touch starting there is not its own.
    let result = view.handle_touch(
        &common::touch_down(1, 0),
        &ctx,
        &mut tree,
        &mut (),
        &mut state,
    );
    assert_eq!(
        result,
        TouchResult::Deferred,
        "a touch on text is for the views around it"
    );
}

#[test]
fn line_limit_truncates_with_an_ellipsis() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 2>::default();
    let text = Text::new("This is a lengthy text here", &font)
        .with_line_limit(1)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "This… ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), "      ");
}

#[test]
fn line_limit_truncates_the_last_allowed_line() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 3>::default();
    let text = Text::new("This is a lengthy text here", &font)
        .with_line_limit(2)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "This  ");
    assert_eq!(buffer.text[1].iter().collect::<String>(), "is a… ");
    assert_eq!(buffer.text[2].iter().collect::<String>(), "      ");
}

#[test]
fn line_limit_leaves_text_that_fits_alone() {
    let font = CharacterBufferFont {};
    let mut buffer = FixedTextBuffer::<6, 1>::default();
    let text = Text::new("Hi", &font)
        .with_line_limit(1)
        .foreground_color(' ');
    make_render_tree(&text, buffer.size(), &mut ()).render(&mut buffer, &' ');
    assert_eq!(buffer.text[0].iter().collect::<String>(), "Hi    ");
}

#[test]
fn truncated_text_claims_the_offered_width() {
    let font = CharacterBufferFont {};
    let text = Text::new("This is a lengthy text here", &font).with_line_limit(1);
    let env = DefaultEnvironment::non_animated();
    let layout = text.layout(&Size::new(6, 5).into(), &env, &mut (), &mut ());
    assert_eq!(layout.resolved_size, Dimensions::new(6, 1));
}

#[test]
fn text_offered_less_than_a_line_still_draws_one() {
    let font = CharacterBufferFont {};
    let text = Text::new("Hi", &font);
    let env = DefaultEnvironment::non_animated();
    let layout = text.layout(&Size::new(6, 0).into(), &env, &mut (), &mut ());
    assert_eq!(layout.resolved_size, Dimensions::new(2, 1));
}
