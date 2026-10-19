//! Whether the pointer pushes into an edge that opens the panel, from how
//! the mouse moves and where the pointer is: one watch, kept where the
//! mouse can be heard whatever window is in front. That is the sensors'
//! process while there is one (elevated, it hears the mouse over elevated
//! windows too, which a process running as the user does not), and tells
//! the rest of Glance each push it sees; else the panel's own thread.

use std::time::Instant;

use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::POINT;

use crate::detector::{Detector, Motion};
use crate::mouse::{edge_contact, monitor_at, Contact, Moved};
use crate::settings::{Edge, Places};

/// Which edges of which screens open a panel to a push (those that hold anything), and how hard the pointer has
/// to push there (none: pushing opens nothing).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Edges {
    pub places: Places,
    pub pressure: Option<i32>,
}

impl Edges {
    /// No edge opening the panel (till Glance says which do).
    pub const fn none() -> Self {
        Edges { places: Places { same: true, every: Vec::new(), screens: Vec::new() }, pressure: None }
    }

    /// As told over the link: no more screens than a desktop has, and a push neither so light that any move opens
    /// the panel nor beyond what a hand gives.
    pub fn checked(mut self) -> Self {
        self.places.screens.truncate(32);
        self.pressure = self.pressure.map(|pressure| pressure.clamp(20, 2000));
        self
    }

    /// Where the pointer at `cursor` is on an edge of its screen that opens the panel: which screen, which edge,
    /// and how hard it has to push there.
    pub fn at(&self, cursor: POINT) -> Option<(Contact, Edge, i32)> {
        let pressure = self.pressure?;
        let here = monitor_at(cursor)?;
        crate::screens::lit(&self.places, here.monitor).each().find_map(|edge| edge_contact(cursor, edge).map(|contact| (contact, edge, pressure)))
    }
}

/// A push that opens the panel: where the pointer was, on which screen, into which edge.
pub type Pushed = (POINT, Contact, Edge);

/// The watch on the edges. Once it has seen a push, the pointer has to leave
/// the edge before it sees another: pushing on is the same gesture.
pub struct Pusher {
    detector: Detector,
    armed: bool,
    /// Where the pointer was at the last look (see `looked`), and whether the mouse's own motion was heard since.
    last_cursor: POINT,
    heard_since: bool,
}

impl Default for Pusher {
    fn default() -> Self {
        Pusher { detector: Detector::default(), armed: true, last_cursor: POINT::default(), heard_since: false }
    }
}

impl Pusher {
    /// The mouse moved (`moved`, as it said itself), the pointer now at `cursor`.
    pub fn heard(&mut self, edges: &Edges, moved: Moved, cursor: POINT, now: Instant) -> Option<Pushed> {
        self.heard_since = true;
        match edges.at(cursor) {
            Some((contact, edge, pressure)) if self.armed && self.detector.motion(moved.toward(edge), now, pressure) => Some(self.pushed(cursor, contact, edge)),
            Some(_) => None,
            None => {
                self.left();
                None
            }
        }
    }

    /// A slow look at the pointer: moved, with nothing heard of the mouse since the last look (a pen, a touch, a
    /// remote session's pointer: none of them can push past the edge), it is taken as a pointer that opens the panel
    /// by resting on the edge.
    pub fn looked(&mut self, edges: &Edges, cursor: POINT, now: Instant) {
        let moved = cursor.x != self.last_cursor.x || cursor.y != self.last_cursor.y;
        if moved && !self.heard_since {
            match edges.at(cursor) {
                Some((_, _, pressure)) if self.armed => {
                    self.detector.motion(Motion::Absolute, now, pressure);
                }
                Some(_) => {}
                None => self.left(),
            }
        }
        self.last_cursor = cursor;
        self.heard_since = false;
    }

    /// A pointer resting on the edge, looked at again: rested long enough, it opens the panel.
    pub fn rested(&mut self, edges: &Edges, cursor: POINT, now: Instant) -> Option<Pushed> {
        match edges.at(cursor) {
            Some((contact, edge, _)) if self.armed && self.detector.dwell_elapsed(now) => Some(self.pushed(cursor, contact, edge)),
            Some(_) => None,
            None => {
                self.left();
                None
            }
        }
    }

    /// Whether a pointer rests on the edge, and wants looking at often (see `rested`).
    pub fn resting(&self) -> bool {
        self.detector.dwelling()
    }

    fn pushed(&mut self, cursor: POINT, contact: Contact, edge: Edge) -> Pushed {
        self.detector.reset();
        self.armed = false;
        (cursor, contact, edge)
    }

    fn left(&mut self) {
        self.detector.reset();
        self.armed = true;
    }
}
