//! Index arithmetic for the tab, agent and space rings. Nothing here talks to
//! herdr: modes.rs fetches the rows, asks these functions for an index, then
//! focuses whatever sits there.

/// `i` folded into `0..n`. Negative values count back from the end, so
/// `wrap(-1, 3)` is 2. `n` must not be zero.
pub fn wrap(i: i64, n: usize) -> usize {
    let n = n as i64;
    (((i % n) + n) % n) as usize
}

/// The row `delta` steps from `cur`, wrapping at either end. With no current
/// row the step enters the list at the end the direction implies: forward
/// lands on row 0, backward on the last row. `n` must not be zero.
pub fn step(cur: Option<usize>, delta: i64, n: usize) -> usize {
    match cur {
        Some(cur) => wrap(cur as i64 + delta, n),
        None if delta > 0 => 0,
        None => n - 1,
    }
}

/// Walk from `cur` in the `delta` direction, once around the list, and return
/// the first row `wants` accepts. `cur` itself is checked last, after the
/// wrap. With no current row the walk starts just outside the near end, so
/// the first step lands on that end row instead of skipping it.
pub fn find_attention<T>(
    rows: &[T],
    cur: Option<usize>,
    delta: i64,
    wants: impl Fn(&T) -> bool,
) -> Option<usize> {
    let n = rows.len();
    if n == 0 {
        return None;
    }
    let outside = if delta > 0 { -1 } else { n as i64 };
    let from = cur.map_or(outside, |i| i as i64);
    (1..=n as i64)
        .map(|s| wrap(from + delta * s, n))
        .find(|&i| wants(&rows[i]))
}

/// Where to ask `tab.move` to put the tab at `cur`, as
/// `(insert_index, landed_index)`. The server evaluates `insert_index`
/// against the list that still contains the moved tab, so moving right has
/// to clear its own slot: `cur + 2`, landing on `cur + 1`. Moving left is
/// `cur - 1` for both. `None` when the move would leave the list.
pub fn tab_move(cur: usize, delta: i64, n: usize) -> Option<(usize, usize)> {
    let cur = cur as i64;
    let (insert, landed) = if delta > 0 {
        (cur + 2, cur + 1)
    } else {
        (cur - 1, cur - 1)
    };
    if insert < 0 || insert > n as i64 {
        return None;
    }
    Some((insert as usize, landed as usize))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_folds_negative_and_overflowing_indexes() {
        assert_eq!(wrap(0, 3), 0);
        assert_eq!(wrap(2, 3), 2);
        assert_eq!(wrap(3, 3), 0);
        assert_eq!(wrap(7, 3), 1);
        assert_eq!(wrap(-1, 3), 2);
        assert_eq!(wrap(-3, 3), 0);
        assert_eq!(wrap(-4, 3), 2);
        assert_eq!(wrap(-1, 1), 0);
    }

    #[test]
    fn step_moves_and_wraps() {
        assert_eq!(step(Some(0), 1, 3), 1);
        assert_eq!(step(Some(2), 1, 3), 0);
        assert_eq!(step(Some(0), -1, 3), 2);
        assert_eq!(step(Some(1), -1, 3), 0);
    }

    #[test]
    fn step_from_none_enters_at_the_near_end() {
        assert_eq!(step(None, 1, 4), 0);
        assert_eq!(step(None, -1, 4), 3);
        assert_eq!(step(None, 1, 1), 0);
        assert_eq!(step(None, -1, 1), 0);
    }

    fn wanted(rows: &[bool], cur: Option<usize>, delta: i64) -> Option<usize> {
        find_attention(rows, cur, delta, |w| *w)
    }

    #[test]
    fn find_attention_walks_forward_and_backward() {
        let rows = [false, true, false, true];
        assert_eq!(wanted(&rows, Some(0), 1), Some(1));
        assert_eq!(wanted(&rows, Some(1), 1), Some(3));
        assert_eq!(wanted(&rows, Some(3), -1), Some(1));
        assert_eq!(wanted(&rows, Some(2), -1), Some(1));
    }

    #[test]
    fn find_attention_wraps_once_around() {
        let rows = [true, false, false];
        assert_eq!(wanted(&rows, Some(1), 1), Some(0));
        let rows = [false, false, true];
        assert_eq!(wanted(&rows, Some(1), -1), Some(2));
        // The current row is the last one checked, after the wrap.
        let rows = [false, true, false];
        assert_eq!(wanted(&rows, Some(1), 1), Some(1));
        assert_eq!(wanted(&rows, Some(1), -1), Some(1));
    }

    #[test]
    fn find_attention_skips_busy_rows() {
        let rows = [false, false, false, true, false];
        assert_eq!(wanted(&rows, Some(0), 1), Some(3));
        assert_eq!(wanted(&rows, Some(0), -1), Some(3));
    }

    #[test]
    fn find_attention_is_none_when_nothing_waits() {
        let rows = [false, false, false];
        assert_eq!(wanted(&rows, Some(1), 1), None);
        assert_eq!(wanted(&rows, Some(1), -1), None);
        assert_eq!(wanted(&rows, None, 1), None);
        assert_eq!(wanted(&rows, None, -1), None);
        assert_eq!(wanted(&[], None, 1), None);
        assert_eq!(wanted(&[], Some(0), -1), None);
    }

    #[test]
    fn find_attention_from_outside_lands_on_the_end_row() {
        let rows = [true, false, true];
        assert_eq!(wanted(&rows, None, 1), Some(0));
        assert_eq!(wanted(&rows, None, -1), Some(2));
        // The end row is skipped only when it is busy.
        let rows = [false, true, false];
        assert_eq!(wanted(&rows, None, 1), Some(1));
        assert_eq!(wanted(&rows, None, -1), Some(1));
    }

    #[test]
    fn tab_move_in_the_middle() {
        assert_eq!(tab_move(1, 1, 4), Some((3, 2)));
        assert_eq!(tab_move(1, -1, 4), Some((0, 0)));
    }

    #[test]
    fn tab_move_at_the_edges() {
        assert_eq!(tab_move(0, -1, 4), None);
        assert_eq!(tab_move(3, 1, 4), None);
        assert_eq!(tab_move(0, 1, 4), Some((2, 1)));
        assert_eq!(tab_move(2, 1, 4), Some((4, 3)));
        assert_eq!(tab_move(3, -1, 4), Some((2, 2)));
        assert_eq!(tab_move(0, 1, 2), Some((2, 1)));
        assert_eq!(tab_move(1, -1, 2), Some((0, 0)));
    }
}
