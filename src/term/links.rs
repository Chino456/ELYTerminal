use std::ops::RangeInclusive;

use alacritty_terminal::{
    index::{Boundary, Point},
    term::{Term, cell::Flags},
};
use ely_gpui_component::terminal::{Target, links};

use super::prompts;

/// The OSC 8 target at `point`, else a URL in its wrapped line; a prompt's own link is no target.
pub fn link_at<T>(term: &Term<T>, mut point: Point) -> Option<String> {
    let grid = term.grid();
    if point.column.0 > 0 && grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
        point.column -= 1;
    }
    if let Some(link) = grid[point]
        .hyperlink()
        .filter(|link| link.uri() != prompts::LINK)
    {
        return Some(link.uri().to_string());
    }
    let end = term.line_search_right(point);
    let mut cell = term.line_search_left(point);
    let (mut text, mut at) = (String::new(), 0);
    let mut prev = cell;
    loop {
        if cell == point {
            at = text.len();
        }
        let spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
        if !grid[cell].flags.intersects(spacer) {
            text.push(grid[cell].c);
            text.extend(grid[cell].zerowidth().into_iter().flatten());
        }
        if cell >= end {
            break;
        }
        cell = cell.add(term, Boundary::Grid, 1);
        if cell == prev {
            break;
        }
        prev = cell;
    }
    links(&text)
        .into_iter()
        .find_map(|(bytes, target)| match target {
            Target::Url(url) if bytes.contains(&at) => Some(url.to_string()),
            _ => None,
        })
}

/// The cell range of the OSC 8 link or URL at `point`, if any.
pub fn link_range_at<T>(term: &Term<T>, mut point: Point) -> Option<RangeInclusive<Point>> {
    let grid = term.grid();
    if point.column.0 > 0 && grid[point].flags.contains(Flags::WIDE_CHAR_SPACER) {
        point.column -= 1;
    }
    let end = term.line_search_right(point);
    let mut cell = term.line_search_left(point);
    if let Some(link) = grid[point]
        .hyperlink()
        .filter(|link| link.uri() != prompts::LINK)
    {
        let (mut from, mut to, mut in_link) = (point, point, false);
        let mut prev = cell;
        loop {
            if grid[cell].hyperlink().as_ref() == Some(&link) {
                if !in_link {
                    from = cell;
                    in_link = true;
                }
                to = cell;
            } else if in_link {
                if cell > point {
                    break;
                }
                in_link = false;
            }
            if cell >= end {
                break;
            }
            cell = cell.add(term, Boundary::Grid, 1);
            if cell == prev {
                break;
            }
            prev = cell;
        }
        return (from <= to).then_some(from..=to);
    }
    if grid[point].c == ' ' {
        return None;
    }
    let (mut text, mut at, mut map) = (String::new(), 0, Vec::new());
    let mut prev = cell;
    loop {
        if cell == point {
            at = text.len();
        }
        let spacer = Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER;
        if !grid[cell].flags.intersects(spacer) {
            map.push((text.len(), cell));
            text.push(grid[cell].c);
            text.extend(grid[cell].zerowidth().into_iter().flatten());
        }
        if cell >= end {
            break;
        }
        cell = cell.add(term, Boundary::Grid, 1);
        if cell == prev {
            break;
        }
        prev = cell;
    }
    links(&text).into_iter().find_map(|(bytes, target)| {
        if matches!(target, Target::Url(_)) && bytes.contains(&at) {
            let from = map
                .iter()
                .rev()
                .find(|(b, _)| *b <= bytes.start)
                .map(|(_, p)| *p)?;
            let to = map
                .iter()
                .rev()
                .find(|(b, _)| *b < bytes.end)
                .map(|(_, p)| *p)?;
            (from <= to).then_some(from..=to)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::{
        event::VoidListener,
        index::{Column, Line},
        term::{Config, test::TermSize},
        vte::ansi::Processor,
    };

    use super::*;

    fn link(bytes: &str, line: i32, column: usize) -> Option<String> {
        let mut term = Term::new(Config::default(), &TermSize::new(12, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes.as_bytes());
        link_at(&term, Point::new(Line(line), Column(column)))
    }

    #[test]
    fn an_osc_8_link_names_its_target() {
        let text = "a \x1b]8;;https://ely.dev/x\x1b\\ely\x1b]8;;\x1b\\ b";
        assert_eq!(link(text, 0, 3).as_deref(), Some("https://ely.dev/x"));
        assert_eq!(link(text, 0, 0), None);
    }

    #[test]
    fn a_prompt_s_own_link_is_no_target() {
        let text = "\x1b]8;;\x7f\x1b\\$ https://ely.dev\x1b]8;;\x1b\\";
        assert_eq!(link(text, 0, 0), None);
        assert_eq!(link(text, 0, 4).as_deref(), Some("https://ely.dev"));
    }

    #[test]
    fn a_url_ends_before_its_sentence_does() {
        assert_eq!(link("(a.io/x).", 0, 2), None, "no scheme, no link");
        let text = "(http://a.b)";
        assert_eq!(link(text, 0, 4).as_deref(), Some("http://a.b"));
        assert_eq!(link(text, 0, 11), None);
    }

    #[test]
    fn a_url_keeps_its_combining_marks() {
        let text = "https://e.test/cafe\u{301}/x";
        assert_eq!(link(text, 1, 1).as_deref(), Some(text));
    }

    #[test]
    fn a_url_runs_across_wrapped_rows_and_wide_cells() {
        let text = "\u{4e2d} https://ely.dev/abc";
        assert_eq!(link(text, 1, 3).as_deref(), Some("https://ely.dev/abc"));
        assert_eq!(link(text, 0, 3).as_deref(), Some("https://ely.dev/abc"));
        assert_eq!(link(text, 0, 1), None);
        let wide = "file://a/\u{4e2d} x";
        assert_eq!(
            link(wide, 0, 10).as_deref(),
            Some("file://a/\u{4e2d}"),
            "a wide cell's spacer is its char"
        );
    }

    #[test]
    fn link_range_at_finds_range_for_urls_and_osc8() {
        let mut term = Term::new(Config::default(), &TermSize::new(40, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(
            &mut term,
            b"see https://ely.dev here\r\nand \x1b]8;;https://a.b\x1b\\link\x1b]8;;\x1b\\ now",
        );
        assert_eq!(
            link_range_at(&term, Point::new(Line(0), Column(8))),
            Some(Point::new(Line(0), Column(4))..=Point::new(Line(0), Column(18)))
        );
        assert_eq!(
            link_range_at(&term, Point::new(Line(1), Column(5))),
            Some(Point::new(Line(1), Column(4))..=Point::new(Line(1), Column(7)))
        );
        assert_eq!(link_range_at(&term, Point::new(Line(0), Column(0))), None);
    }

    #[test]
    fn test_every_column() {
        let mut term = Term::new(Config::default(), &TermSize::new(80, 24), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(
            &mut term,
            b"PS C:\\Users> https://github.com/foo/bar\r\n\x1b]8;;https://test.com\x1b\\click here\x1b]8;;\x1b\\",
        );
        for line in 0..24 {
            for col in 0..80 {
                let p = Point::new(Line(line), Column(col));
                let _ = link_range_at(&term, p);
            }
        }
    }
}
