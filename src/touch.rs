//! Gesture decisions for Touch Mode (ADR 0015), kept free of GTK so they can be tested.

/// What a released row swipe does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwipeEnd {
    /// Slide the row out and remove its entry (with Undo).
    Remove,
    /// Spring back into place.
    Restore,
}

/// Fraction of the row width past which a released swipe removes the entry.
const REMOVE_FRACTION: f64 = 0.4;
/// A fling (release speed in px/s, outward) removes a swipe that is at least this far across.
const FLING_SPEED: f64 = 800.0;
const FLING_MIN_FRACTION: f64 = 0.1;

/// Decide a released row swipe from its offset (px, signed), the row width and the
/// horizontal release velocity (px/s, signed).
pub fn swipe_end(offset: f64, width: f64, velocity: f64) -> SwipeEnd {
    if width <= 0.0 {
        return SwipeEnd::Restore;
    }
    let across = offset.abs() / width;
    let outward_fling = velocity.abs() >= FLING_SPEED && velocity.signum() == offset.signum();
    if across >= REMOVE_FRACTION || (outward_fling && across >= FLING_MIN_FRACTION) {
        SwipeEnd::Remove
    } else {
        SwipeEnd::Restore
    }
}

/// How a touch drag that started on a queue row (not its grip) should be treated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragIntent {
    /// Not far enough to tell; keep watching.
    Undecided,
    /// Clearly sideways: the row follows the finger.
    Swipe,
    /// Anything else belongs to the list's scrolling.
    Scroll,
}

/// Movement (px) before a drag is classified.
const DRAG_SLOP: f64 = 12.0;
/// Sideways movement must dominate vertical by this factor to be a swipe.
const SWIPE_DOMINANCE: f64 = 1.5;

/// Classify a drag from its offset since the press.
pub fn drag_intent(dx: f64, dy: f64) -> DragIntent {
    if dx.hypot(dy) < DRAG_SLOP {
        DragIntent::Undecided
    } else if dx.abs() >= dy.abs() * SWIPE_DOMINANCE {
        DragIntent::Swipe
    } else {
        DragIntent::Scroll
    }
}

/// A skip requested by flinging the cover.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    Next,
    Previous,
}

/// Minimum horizontal fling speed (px/s) on the cover.
const SKIP_SPEED: f64 = 600.0;

/// Decide a cover fling from its release velocity (px/s). Like a carousel, moving the cover
/// left brings in the next entry.
pub fn cover_swipe(vx: f64, vy: f64) -> Option<Skip> {
    if vx.abs() < SKIP_SPEED || vx.abs() < vy.abs() * SWIPE_DOMINANCE {
        None
    } else if vx < 0.0 {
        Some(Skip::Next)
    } else {
        Some(Skip::Previous)
    }
}

/// Height (px) of the band at each end of the list where reordering scrolls it.
const EDGE_BAND: f64 = 72.0;
/// Scroll speed (px per frame) at the very edge; beyond it the speed grows to twice this.
const EDGE_SPEED: f64 = 14.0;

/// Pixels to scroll this frame while an entry is dragged with the finger at `y` in a list
/// `height` px tall (negative scrolls up).
pub fn autoscroll_step(y: f64, height: f64) -> f64 {
    // Depth into the band: 0 at its inner edge, 1 at the list edge, up to 2 past it.
    let depth = if y < EDGE_BAND {
        -((EDGE_BAND - y) / EDGE_BAND)
    } else if y > height - EDGE_BAND {
        (y - (height - EDGE_BAND)) / EDGE_BAND
    } else {
        0.0
    };
    depth.clamp(-2.0, 2.0) * EDGE_SPEED
}
