//! Bars drawn from text cells, sized in display columns like the rest of the line.

use crate::config::Bar;

/// A bar `bar.width` cells wide, with one full cell per whole step of `used` out of 100.
/// `None` when `used` isn't a number.
pub fn bar(used: f64, bar: &Bar) -> Option<String> {
    if !used.is_finite() {
        return None;
    }
    let width = usize::from(bar.width);
    // `as` truncates toward zero, and `used` is clamped first, so `full` is at most `width`.
    let full = (used.clamp(0.0, 100.0) / 100.0 * width as f64) as usize;
    Some(bar.full.repeat(full) + &bar.empty.repeat(width - full))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draw(used: f64) -> Option<String> {
        bar(used, &Bar::default())
    }

    #[test]
    fn fills_whole_steps_only() {
        assert_eq!(draw(0.0).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(9.9).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(10.0).as_deref(), Some("█░░░░░░░░░"));
        assert_eq!(draw(63.0).as_deref(), Some("██████░░░░"));
        assert_eq!(draw(99.9).as_deref(), Some("█████████░"));
        assert_eq!(draw(100.0).as_deref(), Some("██████████"));
    }

    #[test]
    fn clamps_odd_values() {
        assert_eq!(draw(-5.0).as_deref(), Some("░░░░░░░░░░"));
        assert_eq!(draw(250.0).as_deref(), Some("██████████"));
        assert_eq!(draw(1e300).as_deref(), Some("██████████"));
        assert_eq!(draw(f64::NAN), None);
        assert_eq!(draw(f64::INFINITY), None);
    }

    #[test]
    fn uses_the_configured_cells_and_width() {
        let custom = Bar {
            width: 4,
            full: "=".to_string(),
            empty: " ".to_string(),
        };
        assert_eq!(bar(50.0, &custom).as_deref(), Some("==  "));
    }
}
