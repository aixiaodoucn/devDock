#[derive(Clone, Copy, Debug)]
pub(crate) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Placement {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

// 所有输入输出均为目标显示器物理像素，避免 Retina 下重复缩放。
pub(crate) fn place(anchor: Rect, work: Rect, scale: f64) -> Placement {
    let gap = 6. * scale;
    let margin_x = gap.min(work.width / 2.);
    let margin_y = gap.min(work.height / 2.);
    let width = (390. * scale).min(work.width - 2. * margin_x).max(1.);
    let height = (560. * scale).min(work.height - 2. * margin_y).max(1.);
    let min_x = work.x + margin_x;
    let min_y = work.y + margin_y;
    let max_x = (work.x + work.width - margin_x - width).max(min_x);
    let max_y = (work.y + work.height - margin_y - height).max(min_y);
    let below = anchor.y + anchor.height + gap;
    let y = if below + height <= work.y + work.height - margin_y {
        below
    } else {
        anchor.y - gap - height
    };
    Placement {
        x: (anchor.x + anchor.width / 2. - width / 2.).clamp(min_x, max_x),
        y: y.clamp(min_y, max_y),
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn centers_below_menu_bar() {
        assert_eq!(
            place(rect(700., 0., 22., 24.), rect(0., 24., 1440., 850.), 1.),
            Placement {
                x: 516.,
                y: 30.,
                width: 390.,
                height: 560.
            }
        );
    }

    #[test]
    fn retina_scales_panel_and_gap_once() {
        assert_eq!(
            place(rect(1400., 0., 44., 48.), rect(0., 48., 2880., 1700.), 2.),
            Placement {
                x: 1032.,
                y: 60.,
                width: 780.,
                height: 1120.
            }
        );
    }

    #[test]
    fn clamps_both_edges_on_negative_coordinate_monitor() {
        let work = rect(-1920., -300., 1920., 1080.);
        assert_eq!(place(rect(-1920., -324., 22., 24.), work, 1.).x, -1914.);
        assert_eq!(place(rect(-22., -324., 22., 24.), work, 1.).x, -396.);
    }

    #[test]
    fn bottom_tray_opens_upward_and_respects_work_area() {
        let result = place(rect(1000., 1040., 22., 40.), rect(0., 0., 1920., 1040.), 1.);
        assert_eq!(result.y, 474.);
        assert!(result.y + result.height <= 1034.);
    }

    #[test]
    fn shrinks_panel_to_small_work_area() {
        assert_eq!(
            place(rect(280., 0., 20., 24.), rect(0., 24., 300., 400.), 1.),
            Placement {
                x: 6.,
                y: 30.,
                width: 288.,
                height: 388.
            }
        );
    }

    #[test]
    fn fractional_scale_stays_inside_offset_monitor() {
        let work = rect(2560., 200., 1600., 900.);
        let result = place(rect(4120., 170., 30., 30.), work, 1.5);
        assert_eq!(result.width, 585.);
        assert!(result.x >= work.x && result.x + result.width <= work.x + work.width);
        assert!(result.y >= work.y && result.y + result.height <= work.y + work.height);
    }
}
