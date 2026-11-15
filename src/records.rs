use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::error::{Error, Result};
use crate::options::Values;

pub const TOKEN: &str = "worktree-init-record";

#[derive(Debug, Clone, PartialEq)]
pub struct Record {
    pub key: String,
    pub admin: String,
    pub slot: Option<u32>,
    pub target: PathBuf,
    pub options: Values,
}

fn directory(common: &Path) -> PathBuf {
    common.join("worktree-init").join("records")
}

fn key_of(admin_dir: &Path) -> Option<String> {
    let text = fs::read_to_string(admin_dir.join(TOKEN)).ok()?;
    Some(text.trim().to_string())
}

fn invalid(path: &Path, reason: &str) -> Error {
    Error::Other(format!("invalid record {}: {reason}", path.display()))
}

fn read(path: &Path) -> Result<Record> {
    let data: Value = serde_json::from_str(&fs::read_to_string(path)?)
        .map_err(|error| invalid(path, &error.to_string()))?;
    let text = |field: &str| {
        data[field]
            .as_str()
            .map(str::to_string)
            .ok_or_else(|| invalid(path, &format!("missing {field}")))
    };
    let options = data["options"]
        .as_object()
        .cloned()
        .ok_or_else(|| invalid(path, "missing options"))?;
    let key = path.file_stem().unwrap_or_default().to_string_lossy();
    Ok(Record {
        key: key.into_owned(),
        admin: text("admin")?,
        slot: data["slot"]
            .as_u64()
            .and_then(|slot| u32::try_from(slot).ok()),
        target: PathBuf::from(text("target")?),
        options,
    })
}

pub fn all_records(common: &Path) -> Result<Vec<Record>> {
    let Ok(listing) = fs::read_dir(directory(common)) else {
        return Ok(Vec::new());
    };
    let mut paths: Vec<PathBuf> = listing
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort();
    paths.iter().map(|path| read(path)).collect()
}

pub fn find(common: &Path, admin_dir: &Path) -> Result<Option<Record>> {
    let Some(key) = key_of(admin_dir).filter(|key| !key.is_empty()) else {
        return Ok(None);
    };
    let path = directory(common).join(format!("{key}.json"));
    if path.is_file() {
        read(&path).map(Some)
    } else {
        Ok(None)
    }
}

fn new_key() -> Result<String> {
    let mut bytes = [0u8; 8];
    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub fn write(
    common: &Path,
    admin_dir: &Path,
    slot: Option<u32>,
    target: &Path,
    options: &Values,
) -> Result<()> {
    let key = match key_of(admin_dir).filter(|key| !key.is_empty()) {
        Some(key) => key,
        None => new_key()?,
    };
    let directory = directory(common);
    fs::create_dir_all(&directory)?;
    let admin = admin_dir.file_name().unwrap_or_default().to_string_lossy();
    let data = json!({
        "admin": admin,
        "slot": slot,
        "target": target.to_string_lossy(),
        "options": options,
    });
    let staged = directory.join(format!("{key}.json.tmp"));
    let body =
        serde_json::to_string_pretty(&data).map_err(|error| Error::Other(error.to_string()))?;
    fs::write(&staged, body + "\n")?;
    fs::rename(&staged, directory.join(format!("{key}.json")))?;
    fs::write(admin_dir.join(TOKEN), format!("{key}\n"))?;
    Ok(())
}

pub fn discard(common: &Path, record: &Record) -> Result<()> {
    match fs::remove_file(directory(common).join(format!("{}.json", record.key))) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
        _ => Ok(()),
    }
}

pub fn alive(common: &Path, record: &Record) -> bool {
    let admin_dir = common.join("worktrees").join(&record.admin);
    if key_of(&admin_dir).as_deref() != Some(record.key.as_str()) {
        return false;
    }
    if admin_dir.join("locked").exists() {
        return true;
    }
    fs::read_to_string(admin_dir.join("gitdir"))
        .is_ok_and(|gitdir| admin_dir.join(gitdir.trim()).exists())
}
