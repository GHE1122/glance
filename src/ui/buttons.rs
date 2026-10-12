//! The buttons a widget and the overlay show while the pointer is over
//! them: the same ones, in the same order, each saying what it does to a
//! pointer that stays on it.

use std::time::Duration;

use super::canvas::{Canvas, Color, Fill, Point};
use super::icons::Icon;
use super::text::Lang;

/// What one does.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Button {
    /// The window in front taken for a game, or no longer.
    Game,
    Settings,
    /// Kept where it is.
    Pin,
    /// Clicks pass through it to what is behind.
    Through,
    Close,
}

/// All of them, as they stand in their row.
pub const ALL: [Button; 5] = [Button::Game, Button::Settings, Button::Pin, Button::Through, Button::Close];

/// A button's width, and the room between two (DIPs).
pub const SIZE: f32 = 22.0;
pub const GAP: f32 = 4.0;

/// How long the pointer stays over what lets clicks through before the one
/// button that ends that shows: a pointer crossing it does not bring it.
pub const LINGER: Duration = Duration::from_millis(500);

impl Button {
    fn icon(self) -> Icon {
        match self {
            Button::Game => Icon::Game,
            Button::Settings => Icon::Settings,
            Button::Pin => Icon::Pin,
            Button::Through => Icon::Pointer,
            Button::Close => Icon::Close,
        }
    }

    /// What it says of itself, `lit` or not, on a widget or on the overlay.
    pub fn says(self, lit: bool, lang: Lang, overlay: bool) -> &'static str {
        let p = |zh, en| lang.pick(zh, en);
        match (self, lit) {
            (Button::Game, false) => p("把最前面的窗口当作游戏，显示它的帧率", "Take the window in front for a game, and show its frame rate"),
            (Button::Game, true) => p("正在显示游戏的帧率，点一下不再显示", "Showing a game's frame rate. Press to stop"),
            (Button::Settings, _) => p("设置", "Settings"),
            (Button::Pin, false) if overlay => p("锁定位置：不能再拖动或缩放", "Lock where it is: no longer moved or sized"),
            (Button::Pin, false) => p("固定位置和大小：不能再拖动或缩放", "Pin where it is: no longer moved or sized"),
            (Button::Pin, true) => p("已固定，点一下取消", "Kept where it is. Press to free it"),
            (Button::Through, false) => p("鼠标穿透：点击会落到后面的窗口上。之后把鼠标停在这里片刻，这个按钮会再出现", "Let clicks through to what is behind. To end it, rest the pointer here a moment: this button comes back"),
            (Button::Through, true) => p("鼠标穿透已开启，点一下关闭", "Clicks pass through. Press to end it"),
            (Button::Close, _) if overlay => p("关闭悬浮窗", "Close the overlay"),
            (Button::Close, _) => p("收起", "Put away"),
        }
    }
}

/// Draws `button` round `centre`: a plate of its own over whatever is under
/// it, light on a light ground and dark on a dark one; lit, inverted.
pub fn paint(frame: &dyn Canvas, centre: Point, button: Button, lit: bool, dark: bool, rim: Color) {
    let (light, deep) = (Color::hex(0xFFFFFF, 0.94), Color::hex(0x1E1E1E, 0.94));
    let (plate, mark) = if dark { (deep, Color::hex(0xFFFFFF, 1.0)) } else { (light, Color::hex(0x1A1A1A, 1.0)) };
    let (ground, ink) = if lit { (mark, plate) } else { (plate, mark) };
    frame.fill_circle(ground, centre, SIZE / 2.0);
    frame.stroke_rounded(Fill::Solid(rim), centre.x - SIZE / 2.0, centre.y - SIZE / 2.0, SIZE, SIZE, SIZE / 2.0, 1.0);
    frame.icon(button.icon(), centre, 13.0, ink);
}
