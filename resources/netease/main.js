// Glance Icon Row, a plugin for BetterNCM (part of Glance, MIT): Glance puts it in with InfLink-rs and takes it out
// with it (see src\netease.rs).
//
// BetterNCM 1.3.4 puts its button beside NetEase's settings button, in the settings button's parent, and makes the
// title bar's icons fit it there: the bar 200 px wide, its first group of icons 160 px. NetEase 3.1 holds its settings
// button in a badge of its own, 20 px wide, so BetterNCM's button lands in the badge, under the settings button, and
// the widths set crowd the icons onto a second row. Where it is so, the button is moved out to just after the badge,
// and the bar and the group are let be as wide as what they hold. Anywhere else nothing is changed.
(() => {
  // (BetterNCM's own rules are plain: these win wherever they are in the page.)
  const widths = document.createElement('style');
  widths.textContent = '[class^=IconBar_], [class^=MiniModeIconBar_] { width: auto !important; }';
  // BetterNCM's button: the settings button it copied, titled so.
  let button = null;
  const place = () => {
    if (!button || !button.isConnected) {
      button = document.querySelector('.cmd-icon-setting[title="BetterNCM"]');
    }
    const badge = button && button.parentElement;
    if (!badge || !badge.classList.contains('cmd-badge')) {
      return;
    }
    badge.after(button);
    if (!widths.isConnected) {
      document.head.appendChild(widths);
    }
  };
  // The button may come after this runs, and NetEase may draw its title bar again: looked for as the page changes.
  new MutationObserver(place).observe(document.documentElement, { childList: true, subtree: true });
  place();
})();
