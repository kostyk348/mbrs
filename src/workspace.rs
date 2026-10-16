//! Project / workspace persistence (JSON — tinyxml2 in the original).
//! Contains the connection, poll groups, SCADA tiles and value names.

use crate::logging::LogConfig;
use crate::modbus::ConnConfig;
use crate::names::Names;
use crate::scada::Tile;
use crate::store::PollGroup;
use anyhow::Result;
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 2;

#[derive(Clone, Serialize, Deserialize)]
pub struct Project {
    #[serde(default = "one")]
    pub version: u32,
    pub conn: ConnConfig,
    pub groups: Vec<PollGroup>,
    #[serde(default)]
    pub tiles: Vec<Tile>,
    #[serde(default)]
    pub names: Names,
    #[serde(default)]
    pub log: LogConfig,
}

fn one() -> u32 {
    FORMAT_VERSION
}

impl Default for Project {
    fn default() -> Self {
        Self {
            version: FORMAT_VERSION,
            conn: ConnConfig::default(),
            groups: vec![PollGroup::new(1)],
            tiles: vec![],
            names: Names::default(),
            log: LogConfig::default(),
        }
    }
}

impl Project {
    pub fn save(&self, path: &str) -> Result<()> {
        let data = serde_json::to_string_pretty(self)?;
        std::fs::write(path, data)?;
        Ok(())
    }

    pub fn load(path: &str) -> Result<Self> {
        let data = read_workspace_text(path)?;
        let mut p: Project = serde_json::from_str(&data)?;
        if p.version == 0 {
            p.version = FORMAT_VERSION;
        }
        Ok(p)
    }
}

/// Читает файл проекта, не придираясь к кодировке.
///
/// Раньше здесь был `read_to_string`, и любой файл, сохранённый «Блокнотом» как UTF-16 или
/// в CP1251, отказывался открываться с ошибкой «stream did not contain valid UTF-8» — хотя
/// JSON внутри нормальный. Теперь: BOM UTF-8/UTF-16 распознаётся, UTF-16 декодируется,
/// остальное читается с потерями (некорректные байты заменяются), и уже это уходит в serde.
fn read_workspace_text(path: &str) -> Result<String> {
    let bytes = std::fs::read(path)?;
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Ok(String::from_utf8_lossy(&bytes[3..]).into_owned());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let units: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return Ok(String::from_utf16_lossy(&units));
    }
    match String::from_utf8(bytes) {
        Ok(s) => Ok(s),
        // Не UTF-8 (например CP1251): не теряем файл целиком — читаем с потерями,
        // а если JSON всё равно не разберётся, ошибку покажет serde, с номером строки.
        Err(e) => Ok(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_workspace_parses() {
        let s = include_str!("../examples/demo.mbw");
        let p: Project = serde_json::from_str(s).expect("demo.mbw must parse");
        assert!(!p.groups.is_empty());
        assert_eq!(p.groups[0].fc, 0x03);
        assert!(!p.tiles.is_empty());
    }

    fn tmp_path(name: &str) -> String {
        let dir = std::env::temp_dir();
        dir.join(name).to_string_lossy().into_owned()
    }

    #[test]
    fn loads_utf16_file_saved_by_notepad() {
        // «Блокнот → Сохранить как → Юникод» даёт UTF-16LE с BOM: раньше такой файл не
        // открывался вообще — read_to_string падал на «stream did not contain valid UTF-8».
        let json = serde_json::to_string(&Project::default()).unwrap();
        let mut bytes = vec![0xFF, 0xFE];
        for u in json.encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let path = tmp_path("mbrs_utf16_test.mbw");
        std::fs::write(&path, &bytes).unwrap();
        let p = Project::load(&path).expect("UTF-16 проект должен открываться");
        assert_eq!(p.version, FORMAT_VERSION);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loads_non_utf8_bytes_instead_of_refusing() {
        // CP1251 (или любой другой не-UTF-8): файл не теряем целиком, порченный байт
        // заменяется, а не превращает проект в «не читается».
        let json = serde_json::to_string(&Project::default()).unwrap();
        let needle = "\"host\":\"";
        let at = json.find(needle).expect("в проекте есть host") + needle.len();
        let mut bytes = json[..at].as_bytes().to_vec();
        bytes.push(0xE8); // 'и' в CP1251 — недопустимый байт для UTF-8
        bytes.extend_from_slice(json[at..].as_bytes());
        let path = tmp_path("mbrs_cp1251_test.mbw");
        std::fs::write(&path, &bytes).unwrap();
        let p = Project::load(&path).expect("не-UTF-8 файл должен открываться с потерями");
        assert_eq!(p.version, FORMAT_VERSION);
        assert!(
            p.conn.host.contains('\u{FFFD}'),
            "порченный байт заменён на U+FFFD"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn roundtrip() {
        let p = Project::default();
        let s = serde_json::to_string(&p).unwrap();
        let q: Project = serde_json::from_str(&s).unwrap();
        assert_eq!(q.groups.len(), p.groups.len());
    }
}
