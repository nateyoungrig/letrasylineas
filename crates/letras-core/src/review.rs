/// Days until a line comes back in Repaso, given its new score and the previous interval.
/// Misses come back right away; solid answers back off geometrically.
pub fn next_interval_days(score: u8, previous: Option<f64>) -> f64 {
    let prev = previous.unwrap_or(0.0);
    match score {
        0 | 1 => 0.0,
        2 => 1.0,
        3 => (prev * 1.2).max(2.0),
        4 => (prev * 2.0).max(4.0),
        _ => (prev * 3.0).max(7.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grows_on_success_and_resets_on_miss() {
        assert_eq!(next_interval_days(5, None), 7.0);
        assert_eq!(next_interval_days(5, Some(7.0)), 21.0);
        assert_eq!(next_interval_days(1, Some(21.0)), 0.0);
        assert_eq!(next_interval_days(3, Some(1.0)), 2.0);
    }
}
