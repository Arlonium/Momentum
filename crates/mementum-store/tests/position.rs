//! Index fractionnaire : `position_between` reste strictement entre ses bornes.

use mementum_store::position_between;
use proptest::prelude::*;

/// Clés valides : alphabet ordonné `0-9A-Za-z`, sans `0` final (sinon rien ne tient sous la clé).
fn key() -> impl Strategy<Value = String> {
    "[0-9A-Za-z]{0,6}[1-9A-Za-z]"
}

proptest! {
    #[test]
    fn position_between_is_ordered(x in key(), y in key()) {
        prop_assume!(x != y);
        let (a, b) = if x < y { (x, y) } else { (y, x) };
        let m = position_between(Some(&a), Some(&b));
        prop_assert!(a < m && m < b, "{a} < {m} < {b}");
        prop_assert!(!m.ends_with('0'), "clé finissant par 0 : {m}");
        let before = position_between(None, Some(&b));
        prop_assert!(before < b);
        let after = position_between(Some(&a), None);
        prop_assert!(after > a);
    }
}

#[test]
fn unbounded_and_serial_insertions() {
    let first = position_between(None, None);
    assert!(!first.is_empty());
    // 200 insertions au même endroit : toujours juste après "5", puis juste avant la borne haute.
    let mut upper = first.clone();
    for _ in 0..200 {
        let m = position_between(Some("5"), Some(&upper));
        assert!("5" < m.as_str() && m < upper, "5 < {m} < {upper}");
        upper = m;
    }
    let mut lower = "5".to_string();
    let top = "6".to_string();
    for _ in 0..200 {
        let m = position_between(Some(&lower), Some(&top));
        assert!(lower < m && m < top, "{lower} < {m} < {top}");
        lower = m;
    }
}
