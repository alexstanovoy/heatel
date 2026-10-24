//! Writes the audio file inside a sound (`vsnd_c`): a WAV or an MP3, as Dota stores it.
//!
//! ```sh
//! cargo run --example sound_extract -- "<...>/game/dota" sounds/ui/abilitylevel.vsnd_c level
//! ```

use heatel::{DirSource, Sound, Vpk};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [game, vsnd, stem] = &args[..] else {
        return Err("usage: sound_extract <game/dota> <...vsnd_c> <output name>".into());
    };
    let src = DirSource::new(game);
    let pak = Vpk::open(&src, "pak01_dir.vpk")?;
    let bytes = pak.read_file(&src, vsnd)?;
    let sound = Sound::parse(&bytes)?;
    let out = format!("{stem}.{}", sound.extension());
    std::fs::write(&out, sound.file())?;
    eprintln!(
        "{out}: {} Hz, {} channels, {:.2} s",
        sound.sample_rate(),
        sound.channels(),
        sound.duration()
    );
    Ok(())
}
