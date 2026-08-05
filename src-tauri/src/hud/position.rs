//! Where the floating island sits on screen.
//!
//! Pure geometry, deliberately free of AppKit so it can be unit-tested: the caller
//! reads the screen metrics and passes them in. The interesting case is a *saved*
//! position that is no longer reachable — e.g. the island was dragged onto an
//! external display that has since been unplugged. Restoring that verbatim would
//! strand the window offscreen with no way to recover but hand-editing config.json,
//! so a saved position is always clamped back onto the visible area.

/// A screen's usable area, in AppKit coordinates (origin bottom-left, y grows up).
/// This is `NSScreen::visibleFrame` — it excludes the menu bar and the Dock.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VisibleFrame {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Gap between the island's bottom edge and the top of the Dock, in the default
/// bottom-center placement.
const BOTTOM_MARGIN: f64 = 12.0;

/// Resolves the island's top-left position in **Tauri** coordinates (origin
/// top-left, y grows down) from an optional saved position.
///
/// `screen_height` is the full physical screen height (`NSScreen::frame().height`),
/// needed to flip between AppKit's bottom-left origin and Tauri's top-left origin.
///
/// With no saved position the island is centered horizontally and sits
/// `BOTTOM_MARGIN` above the Dock. With a saved position, that position is clamped
/// so the whole window stays inside the visible frame.
pub fn resolve_position(
    saved: Option<(f64, f64)>,
    frame: VisibleFrame,
    screen_height: f64,
    size: (f64, f64),
) -> (f64, f64) {
    let (w, h) = size;
    match saved {
        Some((x, y)) => clamp_to_frame(x, y, frame, screen_height, w, h),
        None => {
            let x = frame.x + (frame.width - w) / 2.0;
            // AppKit y of the window's bottom edge -> Tauri y of its top edge.
            let appkit_bottom = frame.y + BOTTOM_MARGIN;
            let y = screen_height - appkit_bottom - h;
            (x, y)
        }
    }
}

/// Clamps a Tauri-coordinate top-left position so the entire `w`×`h` window stays
/// within `frame`. Returns the default bottom-center position's axis when the
/// visible area is somehow smaller than the window, so we never emit a position
/// that puts the window's own origin outside the screen.
fn clamp_to_frame(
    x: f64,
    y: f64,
    frame: VisibleFrame,
    screen_height: f64,
    w: f64,
    h: f64,
) -> (f64, f64) {
    // Horizontal bounds are the same in both coordinate systems.
    let min_x = frame.x;
    let max_x = frame.x + frame.width - w;

    // Vertical: convert the frame's AppKit extent into Tauri's top-down space.
    // The frame's top edge (AppKit maxY) is the smallest allowed Tauri y; its
    // bottom edge (AppKit minY) is the largest, less the window height.
    let min_y = screen_height - (frame.y + frame.height);
    let max_y = screen_height - frame.y - h;

    // `max` before `min` so that when the window is larger than the visible area
    // (max_* < min_*) we pin to the top-left of the frame rather than past it.
    let cx = x.min(max_x).max(min_x);
    let cy = y.min(max_y).max(min_y);
    (cx, cy)
}

/// Padded hit test backing the "cursor is near the island" affordance: is `point`
/// inside `rect` grown by `padding` on every side? `rect` is `(x, y, width, height)`
/// in whatever coordinate space `point` uses.
pub fn is_within_padding(point: (f64, f64), rect: (f64, f64, f64, f64), padding: f64) -> bool {
    let (px, py) = point;
    let (x, y, w, h) = rect;
    px >= x - padding && px <= x + w + padding && py >= y - padding && py <= y + h + padding
}

/// Converts a window's Tauri top-edge y (top-left origin, y grows down) into the
/// AppKit bottom-edge y (bottom-left origin, y grows up) that the cursor is
/// reported in.
///
/// `flip_height` must be the height Tauri itself flips against — the **primary**
/// display's pixel height — not the height of whichever screen currently holds the
/// key window. Getting this wrong offsets the island's hover zone by the difference
/// between the two, which on a mixed-resolution multi-monitor setup means the
/// island never expands on hover. See `hud::panel::tauri_flip_height`.
pub fn appkit_bottom_from_tauri_top(tauri_top: f64, window_height: f64, flip_height: f64) -> f64 {
    flip_height - tauri_top - window_height
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 1440x900 screen with a 25px menu bar and a 70px Dock.
    fn screen() -> (VisibleFrame, f64) {
        (
            VisibleFrame { x: 0.0, y: 70.0, width: 1440.0, height: 805.0 },
            900.0,
        )
    }

    const SIZE: (f64, f64) = (340.0, 88.0);

    #[test]
    fn defaults_to_bottom_center_above_the_dock() {
        let (frame, height) = screen();
        let (x, y) = resolve_position(None, frame, height, SIZE);
        // Centered: (1440 - 340) / 2
        assert_eq!(x, 550.0);
        // Bottom edge 12px above the Dock: 900 - (70 + 12) - 88
        assert_eq!(y, 730.0);
        // The window's bottom edge must clear the Dock.
        assert!(y + SIZE.1 <= height - frame.y);
    }

    #[test]
    fn honors_a_saved_position_that_is_fully_on_screen() {
        let (frame, height) = screen();
        let saved = Some((200.0, 300.0));
        assert_eq!(resolve_position(saved, frame, height, SIZE), (200.0, 300.0));
    }

    #[test]
    fn clamps_a_position_past_the_right_edge() {
        let (frame, height) = screen();
        // x=1400 would put 300px of a 340px-wide window offscreen.
        let (x, _) = resolve_position(Some((1400.0, 300.0)), frame, height, SIZE);
        assert_eq!(x, 1100.0); // 1440 - 340
    }

    #[test]
    fn clamps_a_position_below_the_dock() {
        let (frame, height) = screen();
        let (_, y) = resolve_position(Some((200.0, 880.0)), frame, height, SIZE);
        // Largest Tauri y keeping the window above the Dock: 900 - 70 - 88
        assert_eq!(y, 742.0);
    }

    #[test]
    fn clamps_a_position_above_the_menu_bar() {
        let (frame, height) = screen();
        let (_, y) = resolve_position(Some((200.0, -400.0)), frame, height, SIZE);
        // Smallest Tauri y: below the menu bar => 900 - (70 + 805) = 25
        assert_eq!(y, 25.0);
    }

    #[test]
    fn recovers_a_position_saved_on_a_disconnected_display() {
        // The island was dragged far onto a second monitor to the right that is
        // now gone. Without clamping the window would be invisible and the user
        // would have no way to get it back.
        let (frame, height) = screen();
        let (x, y) = resolve_position(Some((3200.0, 1400.0)), frame, height, SIZE);
        assert!(x >= frame.x && x + SIZE.0 <= frame.x + frame.width);
        assert!(y >= height - (frame.y + frame.height));
        assert!(y + SIZE.1 <= height - frame.y);
    }

    #[test]
    fn recovers_a_position_saved_on_a_display_to_the_left() {
        // Negative x: a display that used to sit left of the built-in screen.
        let (frame, height) = screen();
        let (x, _) = resolve_position(Some((-1800.0, 300.0)), frame, height, SIZE);
        assert_eq!(x, 0.0);
    }

    #[test]
    fn never_returns_a_position_outside_the_visible_frame() {
        let (frame, height) = screen();
        let candidates = [
            (0.0, 0.0), (-5000.0, -5000.0), (5000.0, 5000.0),
            (550.0, 730.0), (1439.0, 899.0),
        ];
        for saved in candidates {
            let (x, y) = resolve_position(Some(saved), frame, height, SIZE);
            assert!(
                x >= frame.x && x + SIZE.0 <= frame.x + frame.width,
                "x {x} out of bounds for saved {saved:?}"
            );
            assert!(
                y >= height - (frame.y + frame.height) && y + SIZE.1 <= height - frame.y,
                "y {y} out of bounds for saved {saved:?}"
            );
        }
    }

    #[test]
    fn a_dragged_position_round_trips_through_save_and_relaunch() {
        // The drag-and-remember loop the user actually sees: `hud_save_position`
        // stores where the island was dropped, and the next launch feeds that back
        // through `resolve_position`. Verified end-to-end against the running app
        // (drag moved config.json's hud_position, and a relaunch reopened the window
        // there); this pins the pure half so a future change to the clamping can't
        // silently start nudging a perfectly valid position on every restart.
        let (frame, height) = screen();
        for dropped in [(300.0, 640.0), (0.0, 25.0), (1100.0, 742.0)] {
            assert_eq!(
                resolve_position(Some(dropped), frame, height, SIZE),
                dropped,
                "an on-screen dropped position must survive a relaunch verbatim"
            );
        }
    }

    #[test]
    fn the_hover_zone_tracks_the_island_through_the_coordinate_flip() {
        // The full round trip the proximity poll performs: place the island, read
        // its Tauri top-left back, flip to AppKit, and hit-test the cursor. A cursor
        // resting on the island's own center must register as near.
        let (frame, height) = screen();
        let (x, y) = resolve_position(None, frame, height, SIZE);
        let bottom = appkit_bottom_from_tauri_top(y, SIZE.1, height);
        let center = (x + SIZE.0 / 2.0, bottom + SIZE.1 / 2.0);
        assert!(
            is_within_padding(center, (x, bottom, SIZE.0, SIZE.1), 48.0),
            "the cursor on the island's center must count as near"
        );
    }

    #[test]
    fn flipping_against_the_wrong_screen_height_moves_the_hover_zone_off_the_island() {
        // Why `hud::panel::tauri_flip_height` must be the *primary* display's height
        // and not `NSScreen::mainScreen`'s (which follows the key window): with a
        // 900pt-high primary and a focused 1440pt-high secondary, flipping against
        // the wrong one puts the hot zone 540pt away — far outside the 48pt padding,
        // so hovering the island would never expand it.
        let (frame, height) = screen();
        let (x, y) = resolve_position(None, frame, height, SIZE);

        let correct = appkit_bottom_from_tauri_top(y, SIZE.1, height);
        let wrong = appkit_bottom_from_tauri_top(y, SIZE.1, 1440.0);
        assert_eq!(wrong - correct, 540.0, "the two flips must actually differ");

        let cursor = (x + SIZE.0 / 2.0, correct + SIZE.1 / 2.0);
        assert!(is_within_padding(cursor, (x, correct, SIZE.0, SIZE.1), 48.0));
        assert!(
            !is_within_padding(cursor, (x, wrong, SIZE.0, SIZE.1), 48.0),
            "the wrong flip height must miss — this is the multi-monitor bug"
        );
    }

    #[test]
    fn pins_to_the_frame_origin_when_the_window_exceeds_the_visible_area() {
        // Degenerate case: a visible area smaller than the island itself. We must
        // still return the frame's own corner, not a position past it.
        let frame = VisibleFrame { x: 0.0, y: 0.0, width: 100.0, height: 50.0 };
        let (x, y) = resolve_position(Some((900.0, 900.0)), frame, 50.0, SIZE);
        assert_eq!(x, 0.0);
        assert_eq!(y, 0.0);
    }

    #[test]
    fn respects_a_non_zero_frame_origin() {
        // Second display placed to the right of the primary one.
        let frame = VisibleFrame { x: 1440.0, y: 0.0, width: 1920.0, height: 1080.0 };
        let (x, _) = resolve_position(None, frame, 1080.0, SIZE);
        assert_eq!(x, 1440.0 + (1920.0 - 340.0) / 2.0);
    }

    const RECT: (f64, f64, f64, f64) = (100.0, 100.0, 340.0, 88.0);

    #[test]
    fn a_point_inside_the_rect_is_near() {
        assert!(is_within_padding((200.0, 150.0), RECT, 48.0));
    }

    #[test]
    fn a_point_just_inside_the_padding_is_near() {
        // 47px left of the rect's left edge, with 48px of padding.
        assert!(is_within_padding((53.0, 150.0), RECT, 48.0));
        // 47px above the rect's top edge (y grows up here).
        assert!(is_within_padding((200.0, 235.0), RECT, 48.0));
    }

    #[test]
    fn a_point_beyond_the_padding_is_not_near() {
        assert!(!is_within_padding((51.0, 150.0), RECT, 48.0));
        assert!(!is_within_padding((200.0, 237.0), RECT, 48.0));
        assert!(!is_within_padding((489.0, 150.0), RECT, 48.0));
        assert!(!is_within_padding((200.0, 51.0), RECT, 48.0));
    }

    #[test]
    fn zero_padding_reduces_to_plain_containment() {
        assert!(is_within_padding((100.0, 100.0), RECT, 0.0));
        assert!(is_within_padding((440.0, 188.0), RECT, 0.0));
        assert!(!is_within_padding((99.0, 100.0), RECT, 0.0));
        assert!(!is_within_padding((441.0, 188.0), RECT, 0.0));
    }
}
