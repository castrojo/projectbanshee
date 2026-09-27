//! Touch Mode gesture decisions (ADR 0015).

use banshee::touch::{
    DragIntent, Skip, SwipeEnd, autoscroll_step, cover_swipe, drag_intent, swipe_end,
};

#[test]
fn a_row_swiped_past_forty_percent_is_removed_and_a_short_swipe_springs_back() {
    // 500 px wide row, released without speed.
    assert_eq!(swipe_end(-210.0, 500.0, 0.0), SwipeEnd::Remove);
    assert_eq!(swipe_end(205.0, 500.0, 0.0), SwipeEnd::Remove);
    assert_eq!(swipe_end(-150.0, 500.0, 0.0), SwipeEnd::Restore);
    assert_eq!(swipe_end(0.0, 500.0, 0.0), SwipeEnd::Restore);
}

#[test]
fn a_quick_fling_removes_a_short_swipe_but_a_fling_back_restores_it() {
    // 20% across, flung outward fast enough.
    assert_eq!(swipe_end(-100.0, 500.0, -1200.0), SwipeEnd::Remove);
    assert_eq!(swipe_end(100.0, 500.0, 1200.0), SwipeEnd::Remove);
    // Same fling back towards the resting position.
    assert_eq!(swipe_end(-100.0, 500.0, 1200.0), SwipeEnd::Restore);
    // A slow drift doesn't count as a fling.
    assert_eq!(swipe_end(-100.0, 500.0, -300.0), SwipeEnd::Restore);
    // A barely-moved row stays even when flicked.
    assert_eq!(swipe_end(-20.0, 500.0, -2000.0), SwipeEnd::Restore);
}

#[test]
fn a_row_drag_becomes_a_swipe_only_when_clearly_sideways() {
    // Too small to tell yet.
    assert_eq!(drag_intent(6.0, 3.0), DragIntent::Undecided);
    // Mostly sideways past the slop: a swipe, either direction.
    assert_eq!(drag_intent(-18.0, 4.0), DragIntent::Swipe);
    assert_eq!(drag_intent(18.0, -6.0), DragIntent::Swipe);
    // Mostly vertical: leave it to scrolling.
    assert_eq!(drag_intent(4.0, 18.0), DragIntent::Scroll);
    // Diagonal isn't sideways enough to steal the scroll.
    assert_eq!(drag_intent(16.0, 14.0), DragIntent::Scroll);
}

#[test]
fn flinging_the_cover_sideways_skips_like_a_carousel() {
    // Content moves left: the next entry comes in from the right.
    assert_eq!(cover_swipe(-900.0, 100.0), Some(Skip::Next));
    assert_eq!(cover_swipe(900.0, -50.0), Some(Skip::Previous));
    // Too slow, or mostly vertical: nothing.
    assert_eq!(cover_swipe(-300.0, 0.0), None);
    assert_eq!(cover_swipe(-900.0, 800.0), None);
}

#[test]
fn reordering_near_an_edge_scrolls_faster_the_closer_the_finger_gets() {
    // 600 px tall list: the middle doesn't scroll.
    assert_eq!(autoscroll_step(300.0, 600.0), 0.0);
    // Near the top scrolls up, near the bottom scrolls down.
    let near_top = autoscroll_step(40.0, 600.0);
    let at_top = autoscroll_step(0.0, 600.0);
    assert!(near_top < 0.0 && at_top < near_top);
    let near_bottom = autoscroll_step(560.0, 600.0);
    let past_bottom = autoscroll_step(640.0, 600.0);
    assert!(near_bottom > 0.0 && past_bottom > near_bottom);
    // Dragging far outside the list doesn't run away.
    assert_eq!(
        autoscroll_step(5000.0, 600.0),
        autoscroll_step(700.0, 600.0)
    );
}
