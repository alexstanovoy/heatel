//! A folder typed relative to the working folder, which is itself a Dota 2 install. This is the
//! only test of its binary because it changes the process's working folder.

use std::fs;

use heatel::locate::{DOTA_FOLDER, PACKAGE, install_in};

#[test]
fn a_relative_picked_folder_is_taken_from_the_working_folder_and_never_gives_an_empty_install() {
    let scratch = std::env::temp_dir().join(format!("heatel-relative-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let dota = scratch.join("steamapps/common").join(DOTA_FOLDER);
    fs::create_dir_all(dota.join("game/dota")).expect("an install");
    fs::write(
        dota.join("game/dota").join(PACKAGE),
        [0x34, 0x12, 0xaa, 0x55, 1, 0, 0, 0],
    )
    .expect("a package directory");
    std::env::set_current_dir(&dota).expect("the install as the working folder");

    for typed in ["game/dota", "game", "../dota 2 beta/game/dota", "."] {
        let install = install_in(std::path::Path::new(typed))
            .unwrap_or_else(|| panic!("{typed:?} should find the install"));
        assert!(
            install.folder.is_absolute(),
            "{typed:?}: {:?}",
            install.folder
        );
        assert!(
            install.folder.ends_with(DOTA_FOLDER),
            "{typed:?}: {:?}",
            install.folder
        );
        assert!(install.game.is_absolute(), "{typed:?}: {:?}", install.game);
    }
    assert_eq!(
        install_in(std::path::Path::new("")),
        None,
        "an empty typed folder"
    );

    std::env::set_current_dir(std::env::temp_dir()).expect("leave the scratch folder");
    let _ = fs::remove_dir_all(&scratch);
}
