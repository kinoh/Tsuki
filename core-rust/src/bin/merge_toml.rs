use std::env;
use std::fs;
use std::path::Path;

use toml::Value;

#[derive(Debug)]
struct Cli {
    base: String,
    overlays: Vec<String>,
    output: String,
    check: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ValueKind {
    Table,
    Array,
    Scalar,
}

fn main() {
    let cli = match parse_cli(env::args().skip(1).collect()) {
        Ok(cli) => cli,
        Err(err) => {
            eprintln!("{}", err);
            print_help();
            std::process::exit(1);
        }
    };

    let mut merged = match read_toml(&cli.base) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("failed to read base: {}", err);
            std::process::exit(1);
        }
    };

    if !matches!(merged, Value::Table(_)) {
        eprintln!("base must be a TOML table");
        std::process::exit(1);
    }

    for overlay_path in &cli.overlays {
        let overlay = match read_toml(overlay_path) {
            Ok(value) => value,
            Err(err) => {
                eprintln!("failed to read overlay '{}': {}", overlay_path, err);
                std::process::exit(1);
            }
        };
        if let Err(err) = merge_value(&mut merged, &overlay, "") {
            eprintln!(
                "overlay '{}' violates merge constraints: {}",
                overlay_path, err
            );
            std::process::exit(2);
        }
    }

    if cli.check {
        return;
    }

    let rendered = match toml::to_string_pretty(&merged) {
        Ok(value) => value,
        Err(err) => {
            eprintln!("failed to render merged toml: {}", err);
            std::process::exit(1);
        }
    };

    if let Some(parent) = Path::new(&cli.output).parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(err) = fs::create_dir_all(parent) {
                eprintln!("failed to create output directory: {}", err);
                std::process::exit(3);
            }
        }
    }
    if let Err(err) = fs::write(&cli.output, rendered) {
        eprintln!("failed to write output '{}': {}", cli.output, err);
        std::process::exit(3);
    }
}

fn parse_cli(args: Vec<String>) -> Result<Cli, String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        print_help();
        std::process::exit(0);
    }

    let mut base: Option<String> = None;
    let mut overlays: Vec<String> = Vec::new();
    let mut output: Option<String> = None;
    let mut check = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--base" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --base")?;
                base = Some(value.clone());
            }
            "--overlay" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --overlay")?;
                overlays.push(value.clone());
            }
            "--output" => {
                i += 1;
                let value = args.get(i).ok_or("missing value for --output")?;
                output = Some(value.clone());
            }
            "--check" => check = true,
            unknown => return Err(format!("unknown arg: {}", unknown)),
        }
        i += 1;
    }

    let base = base.ok_or("--base is required")?;
    if overlays.is_empty() {
        return Err("at least one --overlay is required".to_string());
    }
    let output = output.ok_or("--output is required")?;

    Ok(Cli {
        base,
        overlays,
        output,
        check,
    })
}

fn print_help() {
    println!("merge-toml");
    println!();
    println!("Usage:");
    println!("  merge_toml --base <path> --overlay <path> [--overlay <path> ...] --output <path> [--check]");
    println!();
    println!("Rules:");
    println!("  - overlay keys must already exist in base");
    println!("  - table values are deep-merged");
    println!("  - scalar values are replaced");
    println!("  - array values are replaced");
}

fn read_toml(path: &str) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|err| err.to_string())?;
    toml::from_str::<Value>(&raw).map_err(|err| err.to_string())
}

fn value_kind(value: &Value) -> ValueKind {
    match value {
        Value::Table(_) => ValueKind::Table,
        Value::Array(_) => ValueKind::Array,
        _ => ValueKind::Scalar,
    }
}

fn merge_value(base: &mut Value, overlay: &Value, path: &str) -> Result<(), String> {
    let base_kind = value_kind(base);
    let overlay_kind = value_kind(overlay);

    match (base_kind, overlay_kind) {
        (ValueKind::Table, ValueKind::Table) => {
            let base_table = base
                .as_table_mut()
                .ok_or_else(|| "internal error: table expected".to_string())?;
            let overlay_table = overlay
                .as_table()
                .ok_or_else(|| "internal error: table expected".to_string())?;

            for (key, overlay_child) in overlay_table {
                let next_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                let base_child = base_table
                    .get_mut(key)
                    .ok_or_else(|| format!("unknown key '{}'", next_path))?;
                merge_value(base_child, overlay_child, next_path.as_str())?;
            }
            Ok(())
        }
        (ValueKind::Array, ValueKind::Array) | (ValueKind::Scalar, ValueKind::Scalar) => {
            *base = overlay.clone();
            Ok(())
        }
        _ => Err(format!(
            "type mismatch at '{}': base={:?}, overlay={:?}",
            if path.is_empty() { "<root>" } else { path },
            base_kind,
            overlay_kind
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_rejects_unknown_key() {
        let mut base = toml::from_str::<Value>(
            r#"
            [db]
            path = "./data/core-rust.db"
            "#,
        )
        .expect("base parse should succeed");
        let overlay = toml::from_str::<Value>(
            r#"
            [db]
            unknown = "x"
            "#,
        )
        .expect("overlay parse should succeed");

        let err = merge_value(&mut base, &overlay, "").expect_err("unknown key must fail");
        assert!(err.contains("db.unknown"), "actual err={}", err);
    }

    #[test]
    fn merge_replaces_array_and_scalar() {
        let mut base = toml::from_str::<Value>(
            r#"
            tags = ["a", "b"]
            enabled = true
            "#,
        )
        .expect("base parse should succeed");
        let overlay = toml::from_str::<Value>(
            r#"
            tags = ["x"]
            enabled = false
            "#,
        )
        .expect("overlay parse should succeed");

        merge_value(&mut base, &overlay, "").expect("merge should succeed");
        let rendered = toml::to_string_pretty(&base).expect("render should succeed");
        assert!(rendered.contains("tags = [\"x\"]"), "rendered={}", rendered);
        assert!(rendered.contains("enabled = false"), "rendered={}", rendered);
    }
}
