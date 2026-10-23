//! Fit each drawn screen to the terminal it runs in. The screens are designed in
//! 24-bit colour with a few Unicode marks; terminals without 24-bit colour get
//! the nearest 256-colour palette entries, and the Linux console (no desktop,
//! 16 colours, a font with little beyond box drawing) also gets plain marks.

use ratatui::buffer::Buffer;
use ratatui::style::Color;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Colours {
    /// 24-bit: draw as designed.
    True,
    /// The xterm 256-colour palette.
    Indexed,
    /// The Linux console: 16 colours and a small font.
    Console,
}

impl Colours {
    /// What this terminal shows, from `TERM` and `COLORTERM`.
    pub fn detect() -> Colours {
        let var = |name| std::env::var(name).unwrap_or_default();
        Colours::from_env(&var("TERM"), &var("COLORTERM"))
    }

    pub fn from_env(term: &str, colorterm: &str) -> Colours {
        if term == "linux" {
            Colours::Console
        } else if matches!(colorterm, "truecolor" | "24bit") {
            Colours::True
        } else {
            Colours::Indexed
        }
    }
}

/// Rewrite `buffer` so this terminal can show it.
pub fn fit(buffer: &mut Buffer, colours: Colours) {
    if colours == Colours::True {
        return;
    }
    for cell in &mut buffer.content {
        cell.fg = colour(cell.fg, colours, false);
        cell.bg = colour(cell.bg, colours, true);
        if colours == Colours::Console
            && let Some(plain) = plain(cell.symbol())
        {
            cell.set_symbol(plain);
        }
    }
}

fn colour(colour: Color, colours: Colours, background: bool) -> Color {
    let Color::Rgb(r, g, b) = colour else { return colour };
    match colours {
        Colours::True => colour,
        Colours::Indexed => Color::Indexed(xterm(r, g, b)),
        Colours::Console => console(r, g, b, background),
    }
}

/// The nearest entry in the xterm 6×6×6 colour cube or its grey ramp.
fn xterm(r: u8, g: u8, b: u8) -> u8 {
    let level = |v: u8| {
        if v < 48 {
            0
        } else if v < 115 {
            1
        } else {
            (v - 35) / 40
        }
    };
    let step = |l: u8| if l == 0 { 0 } else { 55 + 40 * l as i32 };
    let (lr, lg, lb) = (level(r), level(g), level(b));
    let cube = 16 + 36 * lr + 6 * lg + lb;
    let grey_level = ((r as u32 + g as u32 + b as u32) / 3).saturating_sub(3) / 10;
    let grey_level = grey_level.min(23) as u8;
    let grey = 232 + grey_level;
    let grey_value = 8 + 10 * grey_level as i32;
    let distance = |(x, y, z): (i32, i32, i32)| {
        (x - r as i32).pow(2) + (y - g as i32).pow(2) + (z - b as i32).pow(2)
    };
    if distance((grey_value, grey_value, grey_value)) < distance((step(lr), step(lg), step(lb))) {
        grey
    } else {
        cube
    }
}

/// The console's 16 colours by role: the greens stay green, warnings turn red,
/// dark lines turn grey, and a dark tint behind text (the selected row) turns
/// blue, which keeps grey and green text on it readable.
fn console(r: u8, g: u8, b: u8, background: bool) -> Color {
    let (r, g, b) = (r as i32, g as i32, b as i32);
    let brightest = r.max(g).max(b);
    let spread = brightest - r.min(g).min(b);
    if brightest < 0x50 {
        if background { Color::Blue } else { Color::DarkGray }
    } else if spread < 0x20 {
        Color::DarkGray
    } else if r > g {
        Color::LightRed
    } else {
        Color::Green
    }
}

/// A mark the console font can draw, for one it likely cannot.
fn plain(symbol: &str) -> Option<&'static str> {
    Some(match symbol {
        "╭" => "┌",
        "╮" => "┐",
        "╰" => "└",
        "╯" => "┘",
        "●" => "*",
        "◐" => "+",
        "○" => "o",
        "✓" => "+",
        "✕" => "x",
        "›" => ">",
        "…" => ".",
        "∼" | "∽" => "~",
        "▏" => "_",
        "↻" => "!",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;
    use ratatui::style::Style;

    #[test]
    fn reads_the_terminal() {
        assert_eq!(Colours::from_env("xterm-kitty", "truecolor"), Colours::True);
        assert_eq!(Colours::from_env("foot", "24bit"), Colours::True);
        assert_eq!(Colours::from_env("xterm-256color", ""), Colours::Indexed);
        assert_eq!(Colours::from_env("linux", ""), Colours::Console);
    }

    #[test]
    fn the_console_gets_its_own_colours_and_marks() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 3, 1));
        let accent = Style::new().fg(Color::Rgb(0xa2, 0xc4, 0xa8));
        buffer.set_string(0, 0, "╭✓x", accent.bg(Color::Rgb(0x2d, 0x3b, 0x30)));
        fit(&mut buffer, Colours::Console);
        let cells = &buffer.content;
        assert_eq!([cells[0].symbol(), cells[1].symbol(), cells[2].symbol()], ["┌", "+", "x"]);
        assert_eq!((cells[0].fg, cells[0].bg), (Color::Green, Color::Blue));

        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer.set_string(0, 0, "!", Style::new().fg(Color::Rgb(0xe3, 0x9b, 0x84)));
        fit(&mut buffer, Colours::Console);
        assert_eq!(buffer.content[0].fg, Color::LightRed);

        let border = Style::new().fg(Color::Rgb(0x39, 0x44, 0x3b));
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer.set_string(0, 0, "─", border);
        fit(&mut buffer, Colours::Console);
        assert_eq!((buffer.content[0].symbol(), buffer.content[0].fg), ("─", Color::DarkGray));
    }

    #[test]
    fn other_terminals_keep_the_marks_and_get_the_nearest_palette_colour() {
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        buffer.set_string(0, 0, "✓", Style::new().fg(Color::Rgb(0xa2, 0xc4, 0xa8)));
        fit(&mut buffer, Colours::Indexed);
        assert_eq!(buffer.content[0].symbol(), "✓");
        assert_eq!(buffer.content[0].fg, Color::Indexed(xterm(0xa2, 0xc4, 0xa8)));
        assert_eq!(xterm(0, 0, 0), 16);
        assert_eq!(xterm(255, 255, 255), 231);
        assert_eq!(xterm(0x80, 0x80, 0x80), 244);
    }
}
