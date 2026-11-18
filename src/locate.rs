//! Where a Dota 2 install is: the Steam folders of this machine, every library their
//! `libraryfolders.vdf` lists, and a folder a person picks by hand. Everything read here is
//! the player's files, so it is untrusted and every read is bounded: a library list of at most
//! [`MAX_LIBRARY_FILE`] bytes, at most [`MAX_LIBRARIES`] libraries, a fixed set of places tried
//! below a picked folder.
//!
//! The places depend on the system, which is a parameter ([`Os`]) so that each system's layout
//! is tested on any machine.

use std::path::{Path, PathBuf};

use crate::keyvalues::{Kv, parse_kv_in, text};

/// The folder Steam installs Dota 2 in, under a library's `steamapps/common`.
pub const DOTA_FOLDER: &str = "dota 2 beta";

/// The package directory of an install, in its `game/dota` folder: the file that says a folder
/// holds Dota 2.
pub const PACKAGE: &str = "pak01_dir.vpk";

/// The largest library list read; Steam's own is a few kilobytes.
pub const MAX_LIBRARY_FILE: u64 = 1 << 20;

/// The most libraries taken from all Steam folders together.
pub const MAX_LIBRARIES: usize = 64;

/// A VPK directory file starts with this signature (little-endian `0x55aa1234`).
const VPK_SIGNATURE: [u8; 4] = [0x34, 0x12, 0xaa, 0x55];

/// The system whose Steam layout is searched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Os {
    /// Steam's folder from the registry or Program Files; nothing under the home folder.
    Windows,
    /// Native Steam, its `~/.steam` links, Flatpak and Snap, under the home folder.
    Linux,
    /// `~/Library/Application Support/Steam`.
    MacOs,
}

impl Os {
    /// The system this build runs on.
    pub const fn current() -> Os {
        if cfg!(windows) {
            Os::Windows
        } else if cfg!(target_os = "macos") {
            Os::MacOs
        } else {
            Os::Linux
        }
    }
}

/// A Dota 2 install found on this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Install {
    /// The `dota 2 beta` folder (or whatever the player's copy is called): what the player is
    /// shown.
    pub folder: PathBuf,
    /// Its `game/dota` folder, which the importer reads.
    pub game: PathBuf,
}

/// The folders Steam may be installed in on `os`, most likely first. `home` is the user's home
/// folder; `registered` the folders the system names for Steam (on Windows the registry's
/// `SteamPath` and `InstallPath`, then the default under Program Files), tried first.
pub fn steam_roots(os: Os, home: Option<&Path>, registered: &[PathBuf]) -> Vec<PathBuf> {
    let under_home: &[&str] = match os {
        Os::Windows => &[],
        Os::Linux => &[
            ".steam/steam",
            ".local/share/Steam",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
            "snap/steam/common/.local/share/Steam",
            ".steam/root",
        ],
        Os::MacOs => &["Library/Application Support/Steam"],
    };
    let home = home.filter(|home| is_local_absolute(home));
    registered
        .iter()
        .filter(|root| is_local_absolute(root))
        .cloned()
        .chain(under_home.iter().filter_map(|rel| Some(home?.join(rel))))
        .collect()
}

/// Whether a path may be looked at without the player asking: it passes both steps below. A
/// relative path would be looked for in whatever folder the program was started from, and a
/// network path (on Windows `\\server\share`, the `\\?\UNC` and device forms) would connect
/// to another machine, which hands over the user's credentials to whoever named it. Every path
/// that comes from a file or the registry goes through this before it reaches the filesystem.
///
/// 1. [`is_well_formed_local_text`]: the path as text, in either system's syntax (the security
///    rule, the same on every host). A path that is not UTF-8 cannot be judged as text; it is
///    a Unix name and goes on to the second step alone.
/// 2. [`is_usable_here`]: what this host takes for a full local path (a plain usability filter,
///    which on Windows also reads the prefix as the system will).
fn is_local_absolute(path: &Path) -> bool {
    path.to_str().is_none_or(is_well_formed_local_text) && is_usable_here(path)
}

/// Step two of [`is_local_absolute`]: `path` is a full path with no `..` step as this host reads
/// it (a Windows path is not one on Linux, nor a Unix path on Windows), and on Windows its
/// prefix is a drive.
fn is_usable_here(path: &Path) -> bool {
    use std::path::{Component, Prefix};
    path.is_absolute()
        && path.components().all(|part| match part {
            Component::ParentDir => false,
            Component::Prefix(prefix) => matches!(prefix.kind(), Prefix::Disk(_)),
            _ => true,
        })
}

/// Step one of [`is_local_absolute`], the security rule, made on the text alone so that it gives
/// the same answer on every system: a Unix path from `/` or a Windows one from a drive letter
/// and a separator, with no `..` step and no control character (no install has one, and the path
/// goes into a log line). A second separator of either kind after a leading `/` is refused:
/// Windows reads a leading `/` as `\`, so `//server/share` and `/\server\share` are network
/// paths and `/\.\C:` a device path.
fn is_well_formed_local_text(text: &str) -> bool {
    let mut chars = text.chars();
    let rooted = match (chars.next(), chars.next(), chars.next()) {
        (Some('/'), Some(second), _) => second != '/' && second != '\\',
        (Some('/'), None, _) => true,
        (Some(drive), Some(':'), Some('\\' | '/')) => drive.is_ascii_alphabetic(),
        _ => false,
    };
    rooted
        && !text.chars().any(char::is_control)
        && !text.split(['/', '\\']).any(|step| step == "..")
}

/// The library folders a `libraryfolders.vdf` lists, in its order: Steam's current format
/// (`"0" { "path" "…" }`) and the old one (`"1" "…"`), backslashes unescaped as Steam writes
/// them on Windows. Text that is not a library list gives none, and so does an entry that is
/// not well formed as a local absolute path in either system's syntax
/// (step one of the rule): such a list is the player's file, but
/// it is not trusted to send the program anywhere. The result is what the list says, the same on
/// every host; [`libraries`] applies the second step (this host's reading) before any of it is
/// looked at, and a caller that looks at these paths itself must do that too.
pub fn library_paths(vdf: &[u8]) -> Vec<PathBuf> {
    let Ok(text) = text(vdf) else {
        return Vec::new();
    };
    // Every key and string of a list this small is charged against this budget.
    let mut budget = crate::Budget::new(4 * MAX_LIBRARY_FILE as usize);
    let Ok(tree) = parse_kv_in(text, &mut budget) else {
        return Vec::new();
    };
    let Some(Kv::Block(list)) = tree
        .get("libraryfolders")
        .or_else(|| tree.get("LibraryFolders"))
    else {
        return Vec::new();
    };
    list.items()
        .iter()
        .filter(|(key, _)| !key.is_empty() && key.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|(_, value)| match value {
            Kv::Text(path) => Some(path.as_str()),
            Kv::Block(_) => match value.get("path") {
                Some(Kv::Text(path)) => Some(path.as_str()),
                _ => None,
            },
        })
        .map(unescape)
        .filter(|path| is_well_formed_local_text(path))
        .map(PathBuf::from)
        .take(MAX_LIBRARIES)
        .collect()
}

/// A KeyValues string as written: `\\` is one backslash, `\"` a quote.
fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match (c, chars.clone().next()) {
            ('\\', Some(next @ ('\\' | '"'))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// The libraries of the Steam folder `root`: itself, then what its library list names (the
/// local ones, see [`library_paths`]).
pub fn libraries(root: &Path) -> Vec<PathBuf> {
    let listed = ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"]
        .iter()
        .find_map(|rel| read_bounded(&root.join(rel), MAX_LIBRARY_FILE))
        .map(|vdf| library_paths(&vdf))
        .unwrap_or_default();
    std::iter::once(root.to_path_buf())
        .chain(
            listed
                .into_iter()
                .filter(|library| is_local_absolute(library)),
        )
        .collect()
}

/// A file's bytes when it is at most `limit` long.
fn read_bounded(path: &Path, limit: u64) -> Option<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= limit).then_some(bytes)
}

/// Every Dota 2 install in the libraries of the Steam folders `roots`, in their order; an
/// install reached twice (`~/.steam/steam` is often a link to `~/.local/share/Steam`) is
/// listed once. At most [`MAX_LIBRARIES`] different libraries are looked into: a library that
/// several Steam folders list does not use up the allowance twice.
pub fn find_installs(roots: &[PathBuf]) -> Vec<Install> {
    let mut libraries_seen: Vec<PathBuf> = Vec::new();
    let mut found: Vec<Install> = Vec::new();
    let mut real_games: Vec<PathBuf> = Vec::new();
    let libraries = roots
        .iter()
        .take(MAX_LIBRARIES)
        .flat_map(|root| libraries(root));
    for library in libraries {
        let real_library = std::fs::canonicalize(&library).unwrap_or_else(|_| library.clone());
        if libraries_seen.contains(&real_library) {
            continue;
        }
        if libraries_seen.len() == MAX_LIBRARIES {
            break;
        }
        libraries_seen.push(real_library);
        let folder = library.join("steamapps").join("common").join(DOTA_FOLDER);
        let Some(install) = install_at(&folder) else {
            continue;
        };
        let real = std::fs::canonicalize(&install.game).unwrap_or_else(|_| install.game.clone());
        if !real_games.contains(&real) {
            real_games.push(real);
            found.push(install);
        }
    }
    found
}

/// The install in `folder` (a `dota 2 beta` folder) when it holds Dota 2's package directory.
fn install_at(folder: &Path) -> Option<Install> {
    let game = folder.join("game").join("dota");
    is_package(&game.join(PACKAGE)).then(|| Install {
        folder: folder.to_path_buf(),
        game,
    })
}

/// Whether `path` is a VPK directory file: a file starting with the VPK signature.
fn is_package(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok_and(|()| head == VPK_SIGNATURE)
}

/// The install in a folder the player picked: the `dota 2 beta` folder itself, its `game` or
/// `game/dota` folder, the Steam library holding it, that library's `steamapps` or
/// `steamapps/common`, or a Steam folder. None when none of these places holds Dota 2. A
/// relative folder is taken from the working folder, so that the places above it exist (and
/// the folder shown to the player is a whole path).
pub fn install_in(picked: &Path) -> Option<Install> {
    if picked.as_os_str().is_empty() {
        return None;
    }
    let picked = without_parent_steps(&std::path::absolute(picked).ok()?);
    let up = |levels: usize| -> Option<PathBuf> {
        let mut path = picked.as_path();
        for _ in 0..levels {
            path = path.parent()?;
        }
        Some(path.to_path_buf())
    };
    let candidates = [
        Some(picked.clone()),
        up(1),
        up(2),
        Some(picked.join(DOTA_FOLDER)),
        Some(picked.join("common").join(DOTA_FOLDER)),
        Some(picked.join("steamapps").join("common").join(DOTA_FOLDER)),
    ];
    candidates
        .into_iter()
        .flatten()
        .find_map(|folder| install_at(&folder))
}

/// `path` with each `..` step taken out together with the folder it leaves, which is what the
/// step means when no link is in the way: the levels above a folder are then the folders the
/// player sees in a file manager.
fn without_parent_steps(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// The `game/dota` folder of a Dota 2 install: the one in `dota_path` when given (any usual level
/// of it, see [`install_in`]), else the first in the libraries of this machine's Steam (its
/// usual places for the user's home; on Windows Program Files: a program that reads the
/// registry passes Steam's own folder to [`steam_roots`] itself).
pub fn find_game_dir(dota_path: Option<&Path>) -> Option<PathBuf> {
    if let Some(install) = dota_path.and_then(install_in) {
        return Some(install.game);
    }
    let env = |name: &str| {
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    };
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    let registered: Vec<PathBuf> = match Os::current() {
        Os::Windows => ["ProgramFiles(x86)", "ProgramFiles"]
            .into_iter()
            .filter_map(|name| Some(env(name)?.join("Steam")))
            .collect(),
        Os::Linux | Os::MacOs => Vec::new(),
    };
    let roots = steam_roots(Os::current(), home.as_deref(), &registered);
    find_installs(&roots)
        .into_iter()
        .next()
        .map(|install| install.game)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A scratch folder of a test's own, removed when it ends.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Tree {
            let path =
                std::env::temp_dir().join(format!("heatel-steam-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).expect("a scratch folder");
            Tree(path)
        }

        /// A Dota install in the library `library`: its package directory, a few bytes.
        fn dota(&self, library: &Path) -> PathBuf {
            let game = library
                .join("steamapps/common")
                .join(DOTA_FOLDER)
                .join("game/dota");
            fs::create_dir_all(&game).expect("an install");
            fs::write(
                game.join(PACKAGE),
                [&VPK_SIGNATURE[..], &[1, 0, 0, 0]].concat(),
            )
            .expect("a package directory");
            library.join("steamapps/common").join(DOTA_FOLDER)
        }

        /// The library list of the Steam folder `root`, naming `libraries`.
        fn list(&self, root: &Path, libraries: &[&Path]) {
            let mut vdf = String::from("\"libraryfolders\"\n{\n");
            for (n, library) in libraries.iter().enumerate() {
                let path = library.display().to_string().replace('\\', "\\\\");
                vdf.push_str(&format!(
                    "\t\"{n}\"\n\t{{\n\t\t\"path\"\t\t\"{path}\"\n\t\t\"apps\"\n\t\t{{\n\t\t\t\"570\"\t\t\"1\"\n\t\t}}\n\t}}\n"
                ));
            }
            vdf.push_str("}\n");
            fs::create_dir_all(root.join("steamapps")).expect("steamapps");
            fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).expect("a library list");
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn folders(installs: &[Install]) -> Vec<PathBuf> {
        installs.iter().map(|i| i.folder.clone()).collect()
    }

    #[test]
    fn linux_finds_dota_in_a_second_library_of_the_native_steam() {
        let tree = Tree::new("linux");
        let home = tree.0.join("home");
        let root = home.join(".local/share/Steam");
        let games = tree.0.join("games");
        tree.list(&root, &[&root, &games]);
        let dota = tree.dota(&games);
        let roots = steam_roots(Os::Linux, Some(&home), &[]);
        assert_eq!(folders(&find_installs(&roots)), vec![dota]);
    }

    #[test]
    fn linux_finds_dota_in_the_flatpak_steam() {
        let tree = Tree::new("flatpak");
        let home = tree.0.join("home");
        let root = home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam");
        tree.list(&root, &[&root]);
        let dota = tree.dota(&root);
        let roots = steam_roots(Os::Linux, Some(&home), &[]);
        let found = find_installs(&roots);
        assert_eq!(folders(&found), vec![dota.clone()]);
        assert_eq!(found[0].game, dota.join("game/dota"));
    }

    #[cfg(unix)]
    #[test]
    fn an_install_reached_through_a_linked_steam_folder_is_listed_once() {
        let tree = Tree::new("linked");
        let home = tree.0.join("home");
        let root = home.join(".local/share/Steam");
        tree.list(&root, &[&root]);
        tree.dota(&root);
        fs::create_dir_all(home.join(".steam")).expect(".steam");
        std::os::unix::fs::symlink(&root, home.join(".steam/steam")).expect("a link");
        let roots = steam_roots(Os::Linux, Some(&home), &[]);
        assert_eq!(find_installs(&roots).len(), 1);
    }

    #[test]
    fn macos_finds_dota_under_application_support() {
        let tree = Tree::new("macos");
        let home = tree.0.join("home");
        let root = home.join("Library/Application Support/Steam");
        let external = tree.0.join("Volumes/External/SteamLibrary");
        tree.list(&root, &[&root, &external]);
        let dota = tree.dota(&external);
        let roots = steam_roots(Os::MacOs, Some(&home), &[]);
        assert_eq!(folders(&find_installs(&roots)), vec![dota]);
    }

    #[test]
    fn windows_follows_the_registered_steam_folder_to_every_library() {
        let tree = Tree::new("windows");
        let registered = tree.0.join("Program Files (x86)/Steam");
        let second = tree.0.join("D/SteamLibrary");
        let third = tree.0.join("E/Games/Steam");
        tree.list(&registered, &[&registered, &second, &third]);
        let dota = tree.dota(&third);
        // A home folder's Linux places are not Windows places.
        let home = tree.0.join("home");
        tree.dota(&home.join(".local/share/Steam"));
        let roots = steam_roots(Os::Windows, Some(&home), &[registered]);
        assert_eq!(folders(&find_installs(&roots)), vec![dota]);
    }

    #[test]
    fn a_windows_library_list_has_its_backslashes_unescaped() {
        let vdf = br#""libraryfolders"
{
	"0"
	{
		"path"		"C:\\Program Files (x86)\\Steam"
		"label"		""
	}
	"1"
	{
		"path"		"D:\\Games\\Steam \"Library\""
	}
}"#;
        assert_eq!(
            library_paths(vdf),
            vec![
                PathBuf::from(r"C:\Program Files (x86)\Steam"),
                PathBuf::from(r#"D:\Games\Steam "Library""#),
            ]
        );
    }

    #[test]
    fn an_old_style_library_list_names_its_folders_directly() {
        let vdf = br#""LibraryFolders" { "TimeNextStatsReport" "1" "ContentStatsID" "2" "1" "D:\\SteamLibrary" }"#;
        assert_eq!(library_paths(vdf), vec![PathBuf::from(r"D:\SteamLibrary")]);
    }

    #[test]
    fn a_damaged_library_list_names_no_library() {
        for vdf in [
            &b"\xff\xfe not text"[..],
            b"{{{{",
            b"\"other\" { \"0\" \"x\" }",
        ] {
            assert!(library_paths(vdf).is_empty(), "{vdf:?}");
        }
    }

    #[test]
    fn a_library_list_names_at_most_the_bounded_number_of_libraries() {
        let mut vdf = String::from("\"libraryfolders\" {");
        for n in 0..MAX_LIBRARIES * 2 {
            vdf.push_str(&format!(" \"{n}\" {{ \"path\" \"/l/{n}\" }}"));
        }
        vdf.push('}');
        assert_eq!(library_paths(vdf.as_bytes()).len(), MAX_LIBRARIES);
    }

    /// A library list with one block entry per `(key, path)`; a `None` path leaves `"path"` out.
    fn list_of(entries: &[(&str, Option<&str>)]) -> String {
        let mut vdf = String::from("\"libraryfolders\" {");
        for (key, path) in entries {
            match path {
                Some(path) => vdf.push_str(&format!(" \"{key}\" {{ \"path\" \"{path}\" }}")),
                None => vdf.push_str(&format!(" \"{key}\" {{ \"label\" \"x\" }}")),
            }
        }
        vdf.push('}');
        vdf
    }

    #[test]
    fn a_library_list_entry_must_be_a_local_absolute_path() {
        let vdf = list_of(&[
            ("0", Some("games/steam")),
            ("1", Some("../steam")),
            ("2", Some("/games/../../steam")),
            ("3", Some(r"\\server\\share\\steam")),
            ("4", Some("//server/share/steam")),
            ("5", Some(r"\\?\\UNC\\server\\share\\steam")),
            ("6", Some(r"\\.\\C:\\steam")),
            ("7", Some(r"C:steam")),
            ("8", Some(r"\steam")),
            ("9", Some(r"D:\\Games\\..\\steam")),
            ("10", Some("/games/steam")),
            ("11", Some(r"D:\\Games\\Steam")),
            ("12", Some("D:/Games/Steam")),
            // Windows reads a leading `/` as `\`: these are a network and a device path.
            ("13", Some(r"/\\server\\share\\steam")),
            ("14", Some(r"/\\.\\C:\\steam")),
            ("15", Some(r"/\\?\\UNC\\server\\share")),
            // No install is at a path with a control character, which also goes into a log line.
            ("16", Some("/games/ste\u{7}am")),
            ("17", Some("/games/ste\u{1b}[2Jam")),
            ("18", Some("D:\\Games\\Ste\u{1}am")),
        ]);
        assert_eq!(
            library_paths(vdf.as_bytes()),
            vec![
                PathBuf::from("/games/steam"),
                PathBuf::from(r"D:\Games\Steam"),
                PathBuf::from("D:/Games/Steam"),
            ]
        );
    }

    #[test]
    fn a_library_list_entry_without_a_path_or_with_an_empty_one_or_key_is_skipped() {
        let mut vdf = list_of(&[
            ("0", None),
            ("1", Some("")),
            ("", Some("/games/under/an/empty/key")),
            ("x", Some("/games/under/a/word")),
            ("2", Some("/games/steam")),
        ]);
        vdf.insert_str(vdf.len() - 1, r#" "3" "" "4" "/games/old/style""#);
        assert_eq!(
            library_paths(vdf.as_bytes()),
            vec![
                PathBuf::from("/games/steam"),
                PathBuf::from("/games/old/style")
            ]
        );
    }

    /// The rule judges every path before it reaches the filesystem, not only those of a library
    /// list: a registered Steam folder is held to the text rule too.
    #[test]
    fn a_registered_steam_folder_with_a_mixed_separator_network_form_is_no_root() {
        let bad = [
            PathBuf::from(r"/\server\share\Steam"),
            PathBuf::from("/steam/ste\u{7}am"),
        ];
        assert_eq!(steam_roots(Os::Linux, None, &bad), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_registered_steam_folder_on_the_network_or_with_a_parent_step_is_no_root() {
        let ok = PathBuf::from(if cfg!(windows) { r"C:\Steam" } else { "/steam" });
        let bad = [
            PathBuf::from("steam"),
            PathBuf::from(if cfg!(windows) {
                r"C:\a\..\Steam"
            } else {
                "/a/../steam"
            }),
            PathBuf::from(r"\\server\share\Steam"),
            PathBuf::from(r"\\?\UNC\server\share\Steam"),
        ];
        let registered: Vec<PathBuf> = bad.iter().cloned().chain([ok.clone()]).collect();
        assert_eq!(steam_roots(Os::Windows, None, &registered), vec![ok]);
    }

    #[test]
    fn an_install_in_a_library_after_duplicate_steam_folders_is_still_found() {
        let tree = Tree::new("duplicates");
        let first = tree.0.join("first");
        let second = tree.0.join("second");
        let others: Vec<PathBuf> = (0..40).map(|n| tree.0.join(format!("other-{n}"))).collect();
        let listed: Vec<&Path> = others.iter().map(PathBuf::as_path).collect();
        tree.list(&first, &listed);
        let dota = tree.dota(&second);
        let roots = vec![first.clone(), first.clone(), second];
        assert_eq!(folders(&find_installs(&roots)), vec![dota]);
    }

    #[test]
    fn a_library_list_larger_than_the_bound_is_not_read() {
        let tree = Tree::new("huge");
        let root = tree.0.join("Steam");
        fs::create_dir_all(root.join("steamapps")).expect("steamapps");
        let library = tree.0.join("lib");
        tree.dota(&library);
        let mut vdf = format!(
            "\"libraryfolders\" {{ \"1\" {{ \"path\" \"{}\" }} }}",
            library.display()
        );
        vdf.push_str(&" ".repeat(MAX_LIBRARY_FILE as usize));
        fs::write(root.join("steamapps/libraryfolders.vdf"), vdf).expect("a list");
        assert_eq!(libraries(&root), vec![root.clone()]);
    }

    #[test]
    fn a_picked_folder_finds_the_install_at_any_usual_level() {
        let tree = Tree::new("picked");
        let library = tree.0.join("SteamLibrary");
        let dota = tree.dota(&library);
        for picked in [
            dota.clone(),
            dota.join("game"),
            dota.join("game/dota"),
            library.clone(),
            library.join("steamapps"),
            library.join("steamapps/common"),
        ] {
            let found = install_in(&picked).map(|i| i.folder);
            assert_eq!(found.as_ref(), Some(&dota), "{}", picked.display());
        }
    }

    #[test]
    fn a_folder_without_a_dota_package_directory_is_no_install() {
        let tree = Tree::new("not-dota");
        let fake = tree.0.join("fake/game/dota");
        fs::create_dir_all(&fake).expect("a folder");
        fs::write(fake.join(PACKAGE), b"not a package").expect("a file");
        assert_eq!(install_in(&tree.0.join("fake")), None);
        assert_eq!(install_in(&tree.0), None);
    }
}
