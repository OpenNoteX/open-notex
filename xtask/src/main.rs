//! Developer tasks. Run with `cargo xtask <command>`.
//!
//! `seed` and `bench` work today. `snap` needs the Rune shell (M0.5), so it
//! explains itself rather than pretending to run.

use std::path::PathBuf;
use std::process::ExitCode;
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
        "snap" => {
            eprintln!("snap: the headless snapshot harness lands with the Rune shell (M0.5).");
            Ok(())
        }
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
  cargo xtask snap"
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
        for path in &paths {
            let note = vault::read_note(path).map_err(|error| error.to_string())?;
            index::upsert_note(&mut conn, &note.file, &note.body, now_ms(), device)
                .map_err(|error| error.to_string())?;
        }
        println!(
            "indexed {} notes into {} in {} ms",
            paths.len(),
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
