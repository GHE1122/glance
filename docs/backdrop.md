# What is behind Glance's glass

The glass and acrylic skins draw the desktop behind the panel and the
widgets. Glance gets it two ways.

## As the panel opens

The panel is still hidden, so the screen inside its area is copied as it is
(GDI `BitBlt`, `backdrop::Capture::take`). Nothing of Glance's needs hiding:
the panel is not on the screen yet.

## While a window of Glance's is up (`ui::scene`)

A widget always, and the panel once it is up and its desktop is taken again
(moved, sized, the settings gone from its front, live refraction), see what
is behind them through a **scene**:

- A window of Glance's own, cloaked (no one sees it; it is in no taskbar or
  window list), owned, with no extended style.
- In it, the system compositor's thumbnails (`DwmRegisterThumbnail`) of every
  visible, uncloaked, unminimised window under the one it serves that meets
  the area, bottom first (thumbnails are drawn in the order they are made),
  each where its window is. The desktop's own window is one of them.
- That window is captured with Windows.Graphics.Capture. A frame comes when
  anything in it changes, none while nothing does.
- A frame is read back (pixels, lightness, digest: what the skins and themes
  use) only when one is wanted, on another thread: as it comes, or at once if
  one came meanwhile. Widgets want one as often as every 250 ms with live
  refraction, every second without; the one awaited after the scene was
  arranged anew, at once.
- The scene is arranged anew when the window moves past its area, goes to
  another screen, or the windows behind change (the widgets' window watch;
  the panel looks every 250 ms with live refraction). Frames from before an
  arrangement are dropped by their time.
- Windows Glance is taking away are left out of every scene: the panel as a
  widget torn off it takes its place, the hint of where a widget would stick.

Found by trying (VM, 2026-10-13):

- The capture refuses windows with `WS_EX_TOOLWINDOW` or `WS_EX_NOACTIVATE`
  ("Could not capture the given window"); cloaked, owned windows with no
  extended style are captured, as are windows placed off the screen.
- Thumbnails of elevated programs' windows are shown to a process running as
  the user.
- Thumbnails carry no window shadows.
- Over a still desktop: one frame in ten seconds. With live refraction over
  a still desktop the app used about 0.3 % of a core more than without.

Because nothing is captured with Glance's windows hidden, Glance never keeps
its windows out of captures (`WDA_EXCLUDEFROMCAPTURE`): screenshots and
recordings show it, and recorders that stop for protected content do not stop
for it.

Still to see on real machines before live refraction is on by default: a video
playing behind a widget (the cost), several screens at different scales, and
animated wallpapers (Wallpaper Engine draws into a window behind the icons).
