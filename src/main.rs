#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

mod db;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = std::path::PathBuf::from("data");
    std::fs::create_dir_all(&data_dir)?;
    #[cfg(unix)]
    std::fs::set_permissions(&data_dir, std::fs::Permissions::from_mode(0o700))?;

    let db_path = data_dir.join("omprint.db");
    let conn = rusqlite::Connection::open(&db_path)?;
    #[cfg(unix)]
    std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600))?;

    conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

    let count = db::run_migrations(&conn)?;
    println!("Migrations complete: {} applied", count);

    Ok(())
}
