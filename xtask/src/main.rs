//! Developer tasks. Run with `cargo xtask <command>`.
//!
//! `seed` and `bench` work on the store. `snap` drives the real app binary
//! headlessly: Rune owns `--scene`, `--snap`, `--tree`, `--theme` and `--scale`,
//! so the snapshots come from the same code path a person runs, not a copy of it.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};
use std::time::Instant;

use notex_core::ItemId;
use notex_store::{db, index, vault};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        usage();
        return ExitCode::from(2);
    };

    let result = match command {
        "seed" => seed(&args[1..]),
        "bench" => bench(&args[1..]),
        "snap" => snap(&args[1..]),
        "help" | "--help" | "-h" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("unknown command: {other}");
            usage();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn usage() {
    eprintln!(
        "usage:
  cargo xtask seed --count N --dir PATH [--db PATH]
  cargo xtask bench --db PATH
  cargo xtask snap [--scene NAME] [--theme light|dark|both] [--scale N] [--out DIR]

`snap` renders every scene by default. Output: DIR/<scene>-<theme>-<scale>x.png
plus the matching .tree.json."
    );
}

fn seed(args: &[String]) -> Result<(), String> {
    let count: usize = value(args, "count")
        .unwrap_or_else(|| "100".to_owned())
        .parse()
        .map_err(|_| "--count must be a number".to_owned())?;
    let dir = PathBuf::from(value(args, "dir").ok_or("--dir is required")?);

    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;

    let started = Instant::now();
    for i in 0..count {
        let id = ItemId::new();
        let batch = i / 50;
        let body = format!(
            "---\nid: {id}\ntags: [seed, batch-{batch}]\n---\n# Seed note {i}\n\n\
             This note exists to exercise the indexer and search. \
             It mentions launch, planning and note {i}.\n\n\
             - [ ] follow up on item {i}\n"
        );
        let path = dir.join(format!("seed-{i:05}.md"));
        vault::write_atomic(&path, &body).map_err(|error| error.to_string())?;
    }
    println!(
        "seeded {count} notes in {} ms at {}",
        started.elapsed().as_millis(),
        dir.display()
    );

    if let Some(db_path) = value(args, "db") {
        let db_path = PathBuf::from(db_path);
        notex_store::db::assert_outside_vault(&db_path, &dir)
            .or_else(|_| {
                // The file does not exist yet, so canonicalize it by hand.
                if db_path.starts_with(&dir) {
                    Err(notex_store::db::StoreError::DatabaseInsideVault(
                        db_path.display().to_string(),
                    ))
                } else {
                    Ok(())
                }
            })
            .map_err(|error| error.to_string())?;

        let started = Instant::now();
        let mut conn = db::open(&db_path).map_err(|error| error.to_string())?;
        let device = "xtask";
        let paths = vault::scan_paths(&dir).map_err(|error| error.to_string())?;
        let mut batch =
            index::Batch::new(&mut conn, now_ms(), device).map_err(|error| error.to_string())?;
        for path in &paths {
            let note = vault::read_note(path).map_err(|error| error.to_string())?;
            batch
                .upsert(&note.file, &note.body)
                .map_err(|error| error.to_string())?;
        }
        let indexed = batch.commit().map_err(|error| error.to_string())?;
        println!(
            "indexed {indexed} notes into {} in {} ms",
            db_path.display(),
            started.elapsed().as_millis()
        );
    }

    Ok(())
}

fn bench(args: &[String]) -> Result<(), String> {
    let db_path = PathBuf::from(value(args, "db").ok_or("--db is required")?);
    let conn = db::open(&db_path).map_err(|error| error.to_string())?;

    let started = Instant::now();
    let count = index::note_count(&conn).map_err(|error| error.to_string())?;
    println!(
        "notes: {count} (count took {} µs)",
        started.elapsed().as_micros()
    );

    for query in ["launch", "planning", "batch", "note"] {
        let started = Instant::now();
        let hits = index::search(&conn, query, 50).map_err(|error| error.to_string())?;
        println!(
            "search {query:?}: {} hits in {} µs",
            hits.len(),
            started.elapsed().as_micros()
        );
    }

    let size = std::fs::metadata(&db_path)
        .map(|meta| meta.len())
        .map_err(|error| error.to_string())?;
    println!("database: {:.1} MiB", size as f64 / (1024.0 * 1024.0));

    Ok(())
}

/// Render scenes headlessly and write a PNG plus an element tree for each.
fn snap(args: &[String]) -> Result<(), String> {
    build_app()?;

    let out = PathBuf::from(value(args, "out").unwrap_or_else(|| "artifacts/snaps".to_owned()));
    let scale = value(args, "scale").unwrap_or_else(|| "2".to_owned());
    std::fs::create_dir_all(&out).map_err(|error| error.to_string())?;

    let themes: Vec<&str> = match value(args, "theme").as_deref() {
        Some("light") => vec!["light"],
        Some("dark") => vec!["dark"],
        Some("both") | None => vec!["light", "dark"],
        Some(other) => return Err(format!("--theme is light, dark or both, not {other:?}")),
    };

    let scenes = match value(args, "scene") {
        Some(scene) => vec![scene],
        None => list_scenes()?,
    };

    let mut written = 0;
    for scene in &scenes {
        for theme in &themes {
            let stem = format!("{scene}-{theme}-{scale}x");
            let png = out.join(format!("{stem}.png"));
            let tree = out.join(format!("{stem}.tree.json"));
            let started = Instant::now();

            let output = app()?
                .arg("--scene")
                .arg(scene)
                .arg("--theme")
                .arg(theme)
                .arg("--scale")
                .arg(&scale)
                .arg("--snap")
                .arg(&png)
                .arg("--tree")
                .arg(&tree)
                .output()
                .map_err(|error| format!("could not run the app: {error}"))?;
            check(&output, scene, theme)?;

            assert_tree(&tree, scene)?;
            println!(
                "{}: {} ms, {} KB",
                stem,
                started.elapsed().as_millis(),
                png_size(&png)? / 1024
            );
            written += 1;
        }
    }

    println!("wrote {written} snapshots to {}", out.display());
    Ok(())
}

/// The app's own scene list, so the harness cannot drift from `SCENES`.
fn list_scenes() -> Result<Vec<String>, String> {
    let output = app()?
        .arg("--list-scenes")
        .output()
        .map_err(|error| format!("could not run the app: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "listing scenes failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let scenes: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect();
    if scenes.is_empty() {
        return Err("the app listed no scenes".to_owned());
    }
    Ok(scenes)
}

fn check(output: &Output, scene: &str, theme: &str) -> Result<(), String> {
    if output.status.success() {
        return Ok(());
    }
    Err(format!(
        "scene {scene} ({theme}) failed: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    ))
}

/// A tree that has a sidebar but no toolbar means the scene rendered a blank
/// frame, which is the failure a PNG alone cannot catch.
fn assert_tree(path: &Path, scene: &str) -> Result<(), String> {
    let tree = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    for key in ["\"sidebar\"", "\"toolbar\"", "\"nav-today\""] {
        if !tree.contains(key) {
            return Err(format!("scene {scene}: {key} is missing from the tree"));
        }
    }
    Ok(())
}

fn png_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| format!("{}: {error}", path.display()))
}

/// The app binary. `xtask` is a developer tool, so running a sibling binary is
/// fine here; the app itself starts no process.
fn app() -> Result<Command, String> {
    Ok(Command::new(app_path()?))
}

fn app_path() -> Result<PathBuf, String> {
    if let Some(bin) = std::env::var_os("OPEN_NOTEX_BIN") {
        return Ok(PathBuf::from(bin));
    }

    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| workspace_root().unwrap_or_default().join("target"));
    for profile in ["debug", "release"] {
        let path = target.join(profile).join("open-notex");
        if path.is_file() {
            return Ok(path);
        }
    }

    Err("build the app first: cargo build -p open-notex".to_owned())
}

/// Builds the app so a snapshot always reflects the current source.
/// `OPEN_NOTEX_BIN` opts out, for a binary built elsewhere.
fn build_app() -> Result<(), String> {
    if std::env::var_os("OPEN_NOTEX_BIN").is_some() {
        return Ok(());
    }

    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "-p", "open-notex", "--quiet"])
        .current_dir(workspace_root()?)
        .status()
        .map_err(|error| format!("could not run cargo: {error}"))?;
    if !status.success() {
        return Err("cargo build -p open-notex failed".to_owned());
    }
    Ok(())
}

fn workspace_root() -> Result<PathBuf, String> {
    // CARGO_MANIFEST_DIR is `xtask/`; the workspace root is its parent.
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "xtask has no parent directory".to_owned())
}

fn value(args: &[String], name: &str) -> Option<String> {
    let flag = format!("--{name}");
    if let Some(index) = args.iter().position(|arg| arg == &flag) {
        return args.get(index + 1).cloned();
    }
    args.iter()
        .find_map(|arg| arg.strip_prefix(&format!("{flag}=")).map(str::to_owned))
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}
