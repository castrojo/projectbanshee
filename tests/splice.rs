//! The Queue list store is updated by splicing only what changed (ADR 0015).

use banshee::queue::splice_range;

#[test]
fn only_the_changed_middle_is_replaced() {
    // Removing from the middle: one item out at its position.
    assert_eq!(splice_range(&[1, 2, 3, 4], &[1, 2, 4]), (2, 1, 0));
    // Moving an entry down one place swaps two items.
    assert_eq!(splice_range(&[1, 2, 3, 4], &[1, 3, 2, 4]), (1, 2, 2));
    // Moving up across three places replaces that span.
    assert_eq!(splice_range(&[1, 2, 3, 4, 5], &[1, 4, 2, 3, 5]), (1, 3, 3));
    // Appending adds at the end; inserting at the front adds at 0.
    assert_eq!(splice_range(&[1, 2], &[1, 2, 3]), (2, 0, 1));
    assert_eq!(splice_range(&[1, 2], &[0, 1, 2]), (0, 0, 1));
    // Clearing removes everything.
    assert_eq!(splice_range(&[1, 2, 3], &[]), (0, 3, 0));
}

#[test]
fn identical_lists_change_nothing() {
    assert_eq!(splice_range(&[1, 2, 3], &[1, 2, 3]), (3, 0, 0));
    assert_eq!(splice_range::<i32>(&[], &[]), (0, 0, 0));
}

#[test]
fn a_changed_item_with_the_same_id_is_replaced() {
    // (id, title): resolved metadata changes an entry in place.
    let old = [(1, "a"), (2, "b"), (3, "c")];
    let new = [(1, "a"), (2, "B"), (3, "c")];
    assert_eq!(splice_range(&old, &new), (1, 1, 1));
}

#[test]
fn repeated_items_never_overlap_prefix_and_suffix() {
    // The same value at both ends must not be counted twice.
    assert_eq!(splice_range(&[7, 7], &[7]), (1, 1, 0));
    assert_eq!(splice_range(&[7], &[7, 7]), (1, 0, 1));
}
