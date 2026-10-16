//! Рабочие пути приложения по умолчанию.
//!
//! Здесь раньше были зашиты линуксовые пути вида `/home/lain/mbrs-workspace.mbw`, и под
//! Windows они разворачивались в несуществующий `C:\\home\\lain\\...`: диалог открытия
//! стартовал в пустой папке, сохранение падало, лог-файл не создавался.

use std::path::PathBuf;

/// Каталог для рабочих файлов: Windows — `%USERPROFILE%\Documents\mbrs`,
/// остальные ОС — `$HOME/mbrs`.
pub fn default_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let base = if cfg!(windows) {
        home.join("Documents")
    } else {
        home
    };
    base.join("mbrs")
}

/// Полный путь к рабочему файлу по умолчанию (`workspace.mbw`, `names.txt`, `log.csv`).
pub fn default_path(name: &str) -> String {
    default_dir().join(name).to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths_are_absolute_and_named() {
        let p = default_path("workspace.mbw");
        assert!(p.ends_with("workspace.mbw"), "неожиданный путь: {p}");
        assert!(
            std::path::Path::new(&p).is_absolute(),
            "путь должен быть абсолютным: {p}"
        );
        assert!(
            !p.contains("/home/lain"),
            "линуксовые пути не должны возвращаться: {p}"
        );
    }
}
