//! Адресный и slave-скан: перебор и сводка результата.
//!
//! Логика перебора вынесена из рабочего потока (`store::do_scan_*`) в чистые
//! функции, потому что протестировать её там, где она была, нельзя: цикл был
//! перемешан с блокировками `SharedHandle` и сетевым вводом-выводом. Здесь нет
//! ни транспорта, ни потоков — только порядок перебора и сводка, а это как раз
//! то, что ломается незаметно.

/// Границы скана: от `start` до `end` включительно, по возрастанию.
///
/// Защита от вырожденных границ: если `end` меньше `start`, скан пуст. В старой
/// реализации цикла это был настоящий убегающий скан — счётчик увеличивался до
/// `u16::MAX`, и мастер опрашивал все 65 536 адресов вместо 10.
#[derive(Clone, Debug)]
pub struct ScanWindow {
    cur: u16,
    end: u16,
    done: bool,
}

impl ScanWindow {
    pub fn new(start: u16, end: u16) -> Self {
        Self {
            cur: start,
            end,
            done: end < start,
        }
    }

    /// Следующее значение диапазона либо `None`, если диапазон исчерпан.
    pub fn next_value(&mut self) -> Option<u16> {
        if self.done {
            return None;
        }
        let v = self.cur;
        if v == self.end {
            self.done = true;
        } else {
            self.cur = v + 1; // переполнения нет: v < end <= u16::MAX
        }
        Some(v)
    }

    /// Сколько значений ещё осталось отдать (для индикатора прогресса).
    pub fn remaining(&self) -> u32 {
        if self.done {
            0
        } else {
            u32::from(self.end - self.cur) + 1
        }
    }
}

/// Сжимает результат скана в непрерывные диапазоны: `[0,1,2,5,7,8]` -> `[(0,2),(5,5),(7,8)]`.
///
/// Именно так результат читает инженер: «отвечают 0–19, 22 и 40–47», а не список из
/// тридцати одинаковых строк. Вход может быть неотсортированным — сортируем сами.
pub fn contiguous(found: &[(u16, String)]) -> Vec<(u16, u16)> {
    let mut addrs: Vec<u16> = found.iter().map(|(a, _)| *a).collect();
    addrs.sort_unstable();
    addrs.dedup();
    let mut out: Vec<(u16, u16)> = Vec::new();
    for a in addrs {
        match out.last_mut() {
            Some((_, hi)) if *hi + 1 == a => *hi = a,
            _ => out.push((a, a)),
        }
    }
    out
}

/// Человекочитаемая сводка найденного: `0–19, 22, 40–47`.
pub fn summary_text(found: &[(u16, String)]) -> String {
    let ranges = contiguous(found);
    if ranges.is_empty() {
        return "—".to_string();
    }
    ranges
        .iter()
        .map(|(lo, hi)| {
            if lo == hi {
                format!("{lo}")
            } else {
                format!("{lo}–{hi}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take(mut w: ScanWindow) -> Vec<u16> {
        let mut v = Vec::new();
        while let Some(x) = w.next_value() {
            v.push(x);
        }
        v
    }

    fn hits(addrs: &[u16]) -> Vec<(u16, String)> {
        addrs.iter().map(|a| (*a, String::new())).collect()
    }

    #[test]
    fn window_is_inclusive_and_ascending() {
        assert_eq!(take(ScanWindow::new(3, 7)), vec![3, 4, 5, 6, 7]);
        assert_eq!(take(ScanWindow::new(5, 5)), vec![5]);
    }

    #[test]
    fn reversed_bounds_scan_nothing_instead_of_running_away() {
        // старая реализация в этом случае уходила висеть до u16::MAX
        assert_eq!(take(ScanWindow::new(9, 2)), Vec::<u16>::new());
        assert_eq!(ScanWindow::new(9, 2).remaining(), 0);
    }

    #[test]
    fn window_terminates_at_the_top_of_the_address_space() {
        assert_eq!(take(ScanWindow::new(65534, 65535)), vec![65534, 65535]);
        assert_eq!(take(ScanWindow::new(65535, 65535)), vec![65535]);
    }

    #[test]
    fn remaining_counts_down_without_overflow() {
        let mut w = ScanWindow::new(65534, 65535);
        assert_eq!(w.remaining(), 2);
        assert_eq!(w.next_value(), Some(65534));
        assert_eq!(w.remaining(), 1);
        assert_eq!(w.next_value(), Some(65535));
        assert_eq!(w.remaining(), 0);
    }

    #[test]
    fn contiguous_groups_runs_and_singletons() {
        assert_eq!(
            contiguous(&hits(&[0, 1, 2, 5, 7, 8])),
            vec![(0, 2), (5, 5), (7, 8)]
        );
        assert_eq!(contiguous(&hits(&[42])), vec![(42, 42)]);
        assert_eq!(contiguous(&[]), Vec::<(u16, u16)>::new());
    }

    #[test]
    fn contiguous_survives_unsorted_and_duplicated_input() {
        assert_eq!(
            contiguous(&hits(&[8, 0, 7, 1, 8, 2, 5])),
            vec![(0, 2), (5, 5), (7, 8)]
        );
        // соседние значения на границе переполнения не склеиваются в петлю
        assert_eq!(contiguous(&hits(&[65535, 0])), vec![(0, 0), (65535, 65535)]);
    }

    #[test]
    fn summary_reads_like_an_engineers_note() {
        assert_eq!(summary_text(&hits(&[0, 1, 2, 5, 7, 8])), "0–2, 5, 7–8");
        assert_eq!(summary_text(&hits(&[42])), "42");
        assert_eq!(summary_text(&[]), "—");
    }
}
