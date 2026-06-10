//! Конвертер INI → YAML панелей rusefui.
//!
//! ```sh
//! cargo run -p rusefi-ini --bin ini-convert-panels -- \
//!   --ini test_data/rusefi_proteus_f7.ini \
//!   --out /tmp/rusefui-panels-preview
//! ```

use std::fs;
use std::path::PathBuf;

use rusefi_ini::convert_ini_path;

fn default_out_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("RUSEFI_UI_PANELS_DIR") {
        return PathBuf::from(dir).join("_manual_convert");
    }
    std::env::temp_dir().join("rusefui-panels-preview")
}

fn main() {
    let mut ini_path = PathBuf::from("test_data/rusefi_proteus_f7.ini");
    let mut out_dir = default_out_dir();

    let args: Vec<String> = std::env::args().collect();
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--ini" => {
                i += 1;
                ini_path = PathBuf::from(&args[i]);
            }
            "--out" => {
                i += 1;
                out_dir = PathBuf::from(&args[i]);
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage: ini-convert-panels [--ini PATH] [--out DIR]\n\
                     Default: test_data/rusefi_proteus_f7.ini → ~/.rusEFI/projects/_manual_convert"
                );
                return;
            }
            other => {
                eprintln!("Unknown argument: {other}");
                std::process::exit(1);
            }
        }
        i += 1;
    }

    let result = convert_ini_path(&ini_path).unwrap_or_else(|e| {
        eprintln!("Convert {}: {e}", ini_path.display());
        std::process::exit(1);
    });

    fs::create_dir_all(&out_dir).unwrap_or_else(|e| {
        eprintln!("Cannot create {}: {e}", out_dir.display());
        std::process::exit(1);
    });

    for (name, content) in &result.files {
        let path = out_dir.join(name);
        fs::write(&path, content).unwrap_or_else(|e| {
            eprintln!("Write {}: {e}", path.display());
            std::process::exit(1);
        });
    }

    let manifest_yaml = serde_yaml::to_string(&result.manifest).expect("manifest yaml");
    fs::write(out_dir.join("manifest.yaml"), &manifest_yaml).unwrap_or_else(|e| {
        eprintln!("Write manifest: {e}");
        std::process::exit(1);
    });

    let manifest_json = serde_json::to_string_pretty(&result.manifest).expect("manifest json");
    fs::write(out_dir.join("manifest.json"), manifest_json).unwrap_or_else(|e| {
        eprintln!("Write manifest json: {e}");
        std::process::exit(1);
    });

    println!(
        "Converted {} panels from {} → {}",
        result.manifest.panel_count,
        ini_path.display(),
        out_dir.display()
    );
}
