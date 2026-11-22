#[cfg(any(target_os = "macos", test))]
use super::WorkArea;

#[cfg(any(target_os = "macos", test))]
#[derive(serde::Deserialize)]
pub(crate) struct AppKitScreen {
    pub frame: [f64; 4],
    pub visible: [f64; 4],
    pub scale: f64,
}

/// Saved winit macOS positions scale their entire logical global origin by the
/// old window's scale. Recover that origin before choosing the current monitor.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn restore_appkit_work_area(
    x: i32,
    y: i32,
    saved_scale: f64,
    primary_height: f64,
    screens: &[AppKitScreen],
) -> Option<(WorkArea, f64)> {
    if !saved_scale.is_finite() || saved_scale <= 0.0 || !primary_height.is_finite() {
        return None;
    }
    let point = [f64::from(x) / saved_scale, f64::from(y) / saved_scale];
    let mut closest: Option<(&AppKitScreen, f64)> = None;
    for screen in screens {
        let [left, bottom, width, height] = screen.frame;
        if !screen.frame.iter().all(|v| v.is_finite())
            || width <= 0.0
            || height <= 0.0
            || !screen.scale.is_finite()
            || screen.scale <= 0.0
        {
            continue;
        }
        let top = primary_height - bottom - height;
        let right = left + width;
        let lower = top + height;
        // Half-open containment makes a shared edge belong to the monitor that
        // starts at that edge, rather than depending on NSScreen array order.
        if point[0] >= left && point[0] < right && point[1] >= top && point[1] < lower {
            closest = Some((screen, 0.0));
            break;
        }
        let dx = (left - point[0]).max(0.0).max(point[0] - right);
        let dy = (top - point[1]).max(0.0).max(point[1] - lower);
        let distance = dx * dx + dy * dy;
        if closest.is_none_or(|(_, previous)| distance < previous) {
            closest = Some((screen, distance));
        }
    }
    let (screen, _) = closest?;
    let [x, y, width, height] = screen.visible;
    appkit_work_area(x, y, width, height, primary_height, screen.scale)
        .map(|area| (area, screen.scale))
}

/// Pinned winit flips AppKit points against the PRIMARY display height, then
/// multiplies the entire global origin and size by the WINDOW backing scale.
#[cfg(any(target_os = "macos", test))]
pub(crate) fn appkit_work_area(
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    primary_height: f64,
    window_scale: f64,
) -> Option<WorkArea> {
    if ![x, y, width, height, primary_height, window_scale]
        .iter()
        .all(|v| v.is_finite())
        || width <= 0.0
        || height <= 0.0
        || primary_height <= 0.0
        || window_scale <= 0.0
    {
        return None;
    }
    let left = (x * window_scale).round();
    let top = ((primary_height - y - height) * window_scale).round();
    let width = (width * window_scale).round();
    let height = (height * window_scale).round();
    if left < i32::MIN as f64
        || left > i32::MAX as f64
        || top < i32::MIN as f64
        || top > i32::MAX as f64
        || width < 1.0
        || width > i32::MAX as f64
        || height < 1.0
        || height > i32::MAX as f64
    {
        return None;
    }
    Some(WorkArea {
        x: left as i32,
        y: top as i32,
        width: width as u32,
        height: height as u32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn screens() -> Vec<AppKitScreen> {
        vec![
            AppKitScreen {
                frame: [0.0, 0.0, 1440.0, 900.0],
                visible: [0.0, 60.0, 1440.0, 815.0],
                scale: 2.0,
            },
            AppKitScreen {
                frame: [-1920.0, -180.0, 1920.0, 1080.0],
                visible: [-1920.0, -140.0, 1920.0, 1040.0],
                scale: 1.0,
            },
        ]
    }
    #[test]
    fn restore_selects_current_scale_from_saved_logical_origin() {
        let (area, scale) = restore_appkit_work_area(-2400, 200, 2.0, 900.0, &screens()).unwrap();
        assert_eq!(scale, 1.0);
        assert_eq!(
            area,
            WorkArea {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1040
            }
        );
        let (area, scale) = restore_appkit_work_area(600, 100, 1.0, 900.0, &screens()).unwrap();
        assert_eq!(scale, 2.0);
        assert_eq!(area.x, 0);
        assert_eq!(area.y, 50);
    }
    #[test]
    fn restore_uses_half_open_shared_edge_and_nearest_remaining_screen() {
        let mut layout = screens();
        layout.reverse();
        assert_eq!(
            restore_appkit_work_area(0, 100, 1.0, 900.0, &layout)
                .unwrap()
                .1,
            2.0
        );
        assert_eq!(
            restore_appkit_work_area(-9000, 100, 1.0, 900.0, &layout)
                .unwrap()
                .1,
            1.0
        );
        assert!(restore_appkit_work_area(0, 0, f64::NAN, 900.0, &layout).is_none());
        assert!(restore_appkit_work_area(0, 0, 0.0, 900.0, &layout).is_none());
        assert!(restore_appkit_work_area(0, 0, 1.0, 900.0, &[]).is_none());
    }
    #[test]
    fn retina_primary_work_area_excludes_menu_bar_and_dock() {
        assert_eq!(
            appkit_work_area(0.0, 60.0, 1440.0, 815.0, 900.0, 2.0),
            Some(WorkArea {
                x: 0,
                y: 50,
                width: 2880,
                height: 1630
            })
        );
    }
    #[test]
    fn mixed_dpi_left_display_scales_global_origin_with_the_window() {
        assert_eq!(
            appkit_work_area(-1920.0, -140.0, 1920.0, 1040.0, 900.0, 1.0),
            Some(WorkArea {
                x: -1920,
                y: 0,
                width: 1920,
                height: 1040
            })
        );
        assert_eq!(
            appkit_work_area(-1920.0, -140.0, 1920.0, 1040.0, 900.0, 2.0),
            Some(WorkArea {
                x: -3840,
                y: 0,
                width: 3840,
                height: 2080
            })
        );
    }
    #[test]
    fn monitor_above_primary_has_negative_top_down_position() {
        assert_eq!(
            appkit_work_area(100.0, 900.0, 1920.0, 1055.0, 900.0, 1.0),
            Some(WorkArea {
                x: 100,
                y: -1055,
                width: 1920,
                height: 1055
            })
        );
    }
    #[test]
    fn invalid_or_overflowing_native_geometry_is_rejected() {
        assert!(appkit_work_area(f64::NAN, 0.0, 100.0, 100.0, 900.0, 2.0).is_none());
        assert!(appkit_work_area(0.0, 0.0, 100.0, 100.0, 900.0, 0.0).is_none());
        assert!(appkit_work_area(f64::MAX, 0.0, 100.0, 100.0, 900.0, 2.0).is_none());
        assert!(appkit_work_area(0.0, 0.0, 0.0, 100.0, 900.0, 2.0).is_none());
    }
}
