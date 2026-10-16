//! Генератор демонстрационного рабочего пространства `examples/demo.mbw`.
//!
//! Зачем: тест `workspace::tests::demo_workspace_parses` читает `examples/demo.mbw`
//! через `include_str!`. Файла в репозитории не было, поэтому `cargo test` на свежем
//! клоне не собирался вовсе. Теперь файл воспроизводим:
//!
//! ```sh
//! cargo run --example gen_demo      # перезаписать examples/demo.mbw
//! ```
//!
//! Демо намеренно маленькое: две группы опроса (holding-регистры и катушки),
//! два тайла SCADA и несколько понятных имён — чтобы было с чего начать
//! `cargo run --release -- examples/demo.mbw`.

use mbrs::colors::ColorMap;
use mbrs::formats::ValueFormat;
use mbrs::modbus::{ConnConfig, FC_READ_COILS, FC_READ_HOLDING};
use mbrs::scada::{Tile, TileKind};
use mbrs::store::{PollGroup, Scale};
use mbrs::workspace::Project;

fn main() -> anyhow::Result<()> {
    let mut p = Project::default();
    p.conn = ConnConfig::default();
    p.conn.unit = 1;
    p.conn.timeout_ms = 1000;

    // Группа 1: 10 holding-регистров, как в «Setup → Read/Write Definition» Modbus Poll
    let mut g1 = PollGroup::new(1);
    g1.name = "Holding 0-9".into();
    g1.fc = FC_READ_HOLDING;
    g1.start = 0;
    g1.count = 10;
    g1.scan_ms = 1000;
    g1.format = ValueFormat::U16;
    g1.color = ColorMap::default();

    // Группа 2: катушки — битовые функции показываются теми же клетками
    let mut g2 = PollGroup::new(2);
    g2.name = "Coils 0-15".into();
    g2.fc = FC_READ_COILS;
    g2.start = 0;
    g2.count = 16;
    g2.scan_ms = 500;
    g2.format = ValueFormat::U16;
    g2.chart = false;

    // Масштабирование демонстрируем на третьей группе
    let mut g3 = PollGroup::new(3);
    g3.name = "Temperature (scaled)".into();
    g3.fc = FC_READ_HOLDING;
    g3.start = 100;
    g3.count = 8;
    g3.scan_ms = 2000;
    g3.scale = Scale {
        x1: 0.0,
        y1: 0.0,
        x2: 1000.0,
        y2: 100.0,
        decimals: 1,
        enabled: true,
    };

    p.groups = vec![g1, g2, g3];

    let mut t1 = Tile::new(1, 0);
    t1.label = "Reg 0".into();
    t1.kind = TileKind::Value;
    let mut t2 = Tile::new(2, 100);
    t2.label = "Temp".into();
    t2.kind = TileKind::Gauge;
    p.tiles = vec![t1, t2];

    // Value Names из Modbus Poll: значение регистра -> текст
    p.names.value.insert(0, "Ready".into());
    p.names.value.insert(1, "Running".into());
    p.names.value.insert(2, "Fault".into());
    // Binary Names: индекс бита -> текст
    p.names.set_bit(0, "Motor on".into());
    p.names.set_bit(1, "Alarm".into());

    p.save("examples/demo.mbw")?;
    println!(
        "examples/demo.mbw записан: групп {}, тайлов {}, версия формата {}",
        p.groups.len(),
        p.tiles.len(),
        p.version
    );
    Ok(())
}
