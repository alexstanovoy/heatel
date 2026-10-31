//! The item and ability scripts of a Dota 2 install list: every item has its icon name, many
//! their ground models, and the heroes' scripts give their abilities.

use heatel::scripts::entries;
use heatel::{DirSource, Vpk, find_game_dir};

fn install() -> (DirSource, Vpk) {
    let dota = std::env::var_os("HEATEL_DOTA_PATH").map(std::path::PathBuf::from);
    let src = DirSource::new(
        find_game_dir(dota.as_deref()).expect("Dota 2 not found; set HEATEL_DOTA_PATH"),
    );
    let pak = Vpk::open(&src, "pak01_dir.vpk").unwrap();
    (src, pak)
}

/// ```sh
/// cargo test --release --test scripts -- --ignored
/// ```
#[test]
#[ignore = "needs a local Dota 2 install"]
fn the_install_lists_its_items_and_abilities() {
    let (src, pak) = install();
    let read = |path: &str| entries(&pak.read_file(&src, path).unwrap()).unwrap();
    let items = read("scripts/npc/items.txt");
    assert!(items.len() > 200, "{} items", items.len());
    let blink = items
        .iter()
        .find(|e| e.name == "item_blink")
        .expect("blink");
    assert!(blink.item && blink.icon_name() == "blink");
    assert!(
        items.iter().filter(|e| e.model.is_some()).count() > 5,
        "ground models"
    );
    let abilities = read("scripts/npc/npc_abilities.txt");
    assert!(abilities.len() > 100, "{} abilities", abilities.len());
    let heroes: usize = pak
        .entries()
        .filter(|e| e.dir() == "scripts/npc/heroes" && e.ext() == "txt")
        .map(|e| entries(&pak.read(&src, e).unwrap()).unwrap().len())
        .sum();
    assert!(heroes > 500, "{heroes} hero abilities");
}
