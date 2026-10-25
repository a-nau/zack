//! Where to place the chooser window. The monitor containing the mouse is
//! preferred, with the primary monitor (then the first monitor) as fallback.
//! Pure math — the crate glue lives in main.rs.

pub struct Display {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub primary: bool,
}

pub fn pick_display(displays: &[Display], mouse: Option<(f32, f32)>) -> Option<&Display> {
    let with_mouse = mouse.and_then(|(mx, my)| {
        displays
            .iter()
            .find(|d| mx >= d.x && mx < d.x + d.w && my >= d.y && my < d.y + d.h)
    });
    with_mouse
        .or_else(|| displays.iter().find(|d| d.primary))
        .or_else(|| displays.first())
}

/// Top-left corner that centers a `win_w` x `win_h` window on `d`.
pub fn centered_position(d: &Display, win_w: f32, win_h: f32) -> (f32, f32) {
    (d.x + (d.w - win_w) / 2.0, d.y + (d.h - win_h) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn two_monitors() -> Vec<Display> {
        vec![
            Display {
                x: 0.0,
                y: 0.0,
                w: 1920.0,
                h: 1080.0,
                primary: true,
            },
            Display {
                x: 1920.0,
                y: 0.0,
                w: 2560.0,
                h: 1440.0,
                primary: false,
            },
        ]
    }

    #[test]
    fn picks_monitor_under_mouse() {
        let displays = two_monitors();
        let d = pick_display(&displays, Some((2000.0, 500.0))).unwrap();
        assert_eq!(d.x, 1920.0);
    }

    #[test]
    fn falls_back_to_primary_when_mouse_unknown_or_outside() {
        let displays = two_monitors();
        assert_eq!(pick_display(&displays, None).unwrap().x, 0.0);
        assert_eq!(
            pick_display(&displays, Some((-99.0, -99.0))).unwrap().x,
            0.0
        );
    }

    #[test]
    fn falls_back_to_first_without_primary_and_none_when_empty() {
        let mut displays = two_monitors();
        displays[0].primary = false;
        assert_eq!(pick_display(&displays, None).unwrap().x, 0.0);
        assert!(pick_display(&[], None).is_none());
    }

    #[test]
    fn centers_within_the_chosen_monitor() {
        let displays = two_monitors();
        let d = &displays[1];
        let (x, y) = centered_position(d, 640.0, 420.0);
        assert_eq!(
            (x, y),
            (1920.0 + (2560.0 - 640.0) / 2.0, (1440.0 - 420.0) / 2.0)
        );
    }
}
