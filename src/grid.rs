//! Геометрия виртуализированной сетки регистров.
//!
//! Чистая логика без egui: сколько строк реально нужно построить в этом кадре.
//! Immediate-mode GUI перестраивает интерфейс каждый кадр, поэтому рисовать
//! 100 000 строк ради 25 видимых — главная причина просадки FPS в табличных
//! инструментах (Modbus Poll показывает до 125 регистров в окне, но в mbrs
//! количество не ограничено). См. `docs/EGUI-PLAN.md`.

/// Диапазон строк `[first, last)` для отрисовки при текущей прокрутке.
///
/// * `total` — всего строк в таблице;
/// * `row_pitch` — шаг строки в пикселях (высота + отступы);
/// * `viewport_h` — высота видимой области;
/// * `scroll_y` — смещение прокрутки в координатах содержимого (`viewport.min.y`);
/// * `overscan` — сколько строк дорисовать сверху и снизу, чтобы не мигало на границе.
///
/// Возвращает полуинтервал; при некорректных входах и при прокрутке за конец —
/// пустой диапазон `(total, total)`, то есть «ничего не рисовать».
pub fn visible_range(
    total: usize,
    row_pitch: f32,
    viewport_h: f32,
    scroll_y: f32,
    overscan: usize,
) -> (usize, usize) {
    if total == 0 || !row_pitch.is_finite() || row_pitch <= 0.0 {
        return (0, 0);
    }
    if !viewport_h.is_finite() || !scroll_y.is_finite() || viewport_h <= 0.0 {
        return (0, 0);
    }
    let start = (scroll_y.max(0.0) / row_pitch).floor() as usize; // первая задетая строка
    let visible = (viewport_h / row_pitch).ceil() as usize + 1; // строк, которые могут быть видны
    let first = start.saturating_sub(overscan).min(total);
    let last = (start + visible + overscan).min(total);
    (first, last.max(first))
}

#[cfg(test)]
mod tests {
    use super::visible_range;

    #[test]
    fn builds_only_visible_rows_not_the_whole_table() {
        // 100 000 строк, видно 600 px при шаге 24 px: строим единицы, а не десятки тысяч
        let (first, last) = visible_range(100_000, 24.0, 600.0, 0.0, 4);
        assert_eq!(first, 0);
        assert!(
            last <= 30,
            "построено {last} строк — виртуализация не работает"
        );
        assert!(last >= 25, "построено {last} строк — не хватает видимых");
    }

    #[test]
    fn follows_scroll_offset() {
        let (first, last) = visible_range(10_000, 24.0, 600.0, 24.0 * 100.0, 4);
        assert_eq!(first, 96); // 100 - overscan
        assert_eq!(last, 130); // 100 + 26 + 4
        assert!(last - first <= 40);
    }

    #[test]
    fn empty_and_degenerate_inputs_are_safe() {
        assert_eq!(visible_range(0, 24.0, 600.0, 0.0, 4), (0, 0));
        assert_eq!(visible_range(100, 0.0, 600.0, 0.0, 4), (0, 0));
        assert_eq!(visible_range(100, -5.0, 600.0, 0.0, 4), (0, 0));
        assert_eq!(visible_range(100, 24.0, 0.0, 0.0, 4), (0, 0));
        assert_eq!(visible_range(100, 24.0, f32::NAN, 0.0, 4), (0, 0));
    }

    #[test]
    fn scrolling_past_the_end_builds_nothing() {
        let (first, last) = visible_range(10, 24.0, 600.0, 100_000.0, 4);
        assert_eq!((first, last), (10, 10));
        assert!(first <= last);
    }

    #[test]
    fn range_is_always_ordered_and_inside_bounds() {
        for scroll in [0.0, 1.0, 23.9, 24.0, 25.0, 600.0, 5_000.0, 1e9] {
            for total in [1usize, 2, 25, 26, 27, 100, 10_000] {
                let (first, last) = visible_range(total, 24.0, 300.0, scroll, 3);
                assert!(
                    first <= last,
                    "неупорядоченный диапазон при scroll={scroll}, total={total}"
                );
                assert!(
                    last <= total,
                    "выход за границы при scroll={scroll}, total={total}"
                );
                assert!(
                    last - first <= (300.0_f32 / 24.0_f32).ceil() as usize + 2 * 3 + 1,
                    "построено слишком много строк при scroll={scroll}, total={total}"
                );
            }
        }
    }
}
