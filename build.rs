use std::path::Path;

fn main() {
    const TORIEL_VOICE_PATH: &str = "src/tts/toriel/voice_toriel.mp3";

    println!("cargo::rustc-check-cfg=cfg(toriel_voice)");
    println!("cargo::rerun-if-changed=src/tts/toriel");

    if Path::new(TORIEL_VOICE_PATH).is_file() {
        println!("cargo::rustc-cfg=toriel_voice");
    }
}
