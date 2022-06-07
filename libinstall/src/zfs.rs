use anyhow::{bail, Result};
use log::{info, warn};
use std::process::Command;

pub(crate) fn zpool_set(pool: &str, n: &str, v: &str) -> Result<()> {
    if pool.contains('/') {
        bail!("no / allowed here");
    }

    info!("SET POOL PROPERTY ON {}: {} = {}", pool, n, v);

    let cmd = Command::new("/sbin/zpool")
        .env_clear()
        .arg("set")
        .arg(&format!("{}={}", n, v))
        .arg(pool)
        .output()?;

    if !cmd.status.success() {
        let errmsg = String::from_utf8_lossy(&cmd.stderr);
        bail!("zpool set {} failed: {}", n, errmsg);
    }

    Ok(())
}

pub(crate) fn zfs_set(dataset: &str, n: &str, v: &str) -> Result<()> {
    info!("SET DATASET PROPERTY ON {}: {} = {}", dataset, n, v);

    let cmd = Command::new("/sbin/zfs")
        .env_clear()
        .arg("set")
        .arg(&format!("{}={}", n, v))
        .arg(dataset)
        .output()?;

    if !cmd.status.success() {
        let errmsg = String::from_utf8_lossy(&cmd.stderr);
        bail!("zfs set {} failed: {}", n, errmsg);
    }

    Ok(())
}

pub(crate) fn zfs_get(dataset: &str, n: &str) -> Result<String> {
    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("get")
        .arg("-H")
        .arg("-o")
        .arg("value")
        .arg(n)
        .arg(dataset)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        bail!("zfs get failed: {}", errmsg);
    }

    let out = String::from_utf8(zfs.stdout)?;
    Ok(out.trim().to_string())
}

pub(crate) fn dataset_exists(dataset: &str) -> Result<bool> {
    if dataset.contains('@') {
        bail!("no @ allowed here");
    }

    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("list")
        .arg("-Ho")
        .arg("name")
        .arg(dataset)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        if errmsg.trim().ends_with("dataset does not exist") {
            return Ok(false);
        }
        bail!("zfs list failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn dataset_remove(dataset: &str) -> Result<bool> {
    if dataset.contains('@') {
        bail!("no @ allowed here");
    }

    info!("DESTROY DATASET: {}", dataset);

    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("destroy")
        .arg("-r")
        .arg(dataset)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        if errmsg.trim().ends_with("dataset does not exist") {
            return Ok(false);
        }
        bail!("zfs destroy failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn pool_destroy(name: &str) -> Result<bool> {
    if name.contains('@') {
        bail!("no @ allowed here");
    }

    info!("DESTROY POOL: {}", name);

    let cmd = Command::new("/sbin/zpool")
        .env_clear()
        .arg("destroy")
        .arg("-f")
        .arg(&name)
        .output()?;

    if !cmd.status.success() {
        let errmsg = String::from_utf8_lossy(&cmd.stderr);
        if errmsg.trim().ends_with("no such pool") {
            return Ok(false);
        }
        bail!("zpool destroy failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn pool_export(name: &str) -> Result<bool> {
    if name.contains('@') {
        bail!("no @ allowed here");
    }

    info!("EXPORT POOL: {}", name);

    loop {
        let cmd = Command::new("/sbin/zpool")
            .env_clear()
            .arg("export")
            .arg(&name)
            .output()?;

        if cmd.status.success() {
            break;
        }

        let errmsg = String::from_utf8_lossy(&cmd.stderr);
        if errmsg.trim().ends_with("pool is busy") {
            warn!("pool is busy... retrying...");
            std::thread::sleep(std::time::Duration::from_secs(1));
            continue;
        }
        bail!("zpool export failed: {}", errmsg);
    }

    Ok(true)
}

#[allow(dead_code)]
pub(crate) fn snapshot_remove(dataset: &str, snapshot: &str) -> Result<bool> {
    if dataset.contains('@') || snapshot.contains('@') {
        bail!("no @ allowed here");
    }

    let n = format!("{}@{}", dataset, snapshot);
    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("destroy")
        .arg(&n)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        if errmsg.trim().ends_with("dataset does not exist") {
            return Ok(false);
        }
        bail!("zfs list failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn snapshot_exists(dataset: &str, snapshot: &str) -> Result<bool> {
    if dataset.contains('@') || snapshot.contains('@') {
        bail!("no @ allowed here");
    }

    let n = format!("{}@{}", dataset, snapshot);
    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("list")
        .arg("-t")
        .arg("snapshot")
        .arg("-Ho")
        .arg("name")
        .arg(&n)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        if errmsg.trim().ends_with("dataset does not exist") {
            return Ok(false);
        }
        bail!("zfs list failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn snapshot_create(dataset: &str, snapshot: &str) -> Result<bool> {
    if dataset.contains('@') || snapshot.contains('@') {
        bail!("no @ allowed here");
    }

    let n = format!("{}@{}", dataset, snapshot);
    info!("CREATE SNAPSHOT: {}", n);

    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("snapshot")
        .arg(&n)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        bail!("zfs snapshot failed: {}", errmsg);
    }

    Ok(true)
}

pub(crate) fn snapshot_rollback(dataset: &str, snapshot: &str) -> Result<bool> {
    if dataset.contains('@') || snapshot.contains('@') {
        bail!("no @ allowed here");
    }

    let n = format!("{}@{}", dataset, snapshot);
    info!("ROLLBACK TO SNAPSHOT: {}", n);

    let zfs = Command::new("/sbin/zfs")
        .env_clear()
        .arg("rollback")
        .arg("-r")
        .arg(&n)
        .output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        bail!("zfs snapshot failed: {}", errmsg);
    }

    Ok(true)
}

fn build_props<S: AsRef<str>>(props: &[(S, S)]) -> Vec<(&str, &str)> {
    props
        .iter()
        .map(|(k, v)| (k.as_ref(), v.as_ref()))
        .collect()
}

pub(crate) fn dataset_create<S: AsRef<str>>(
    dataset: &str,
    parents: bool,
    properties: &[(S, S)],
) -> Result<()> {
    if dataset.contains('@') {
        bail!("no @ allowed here");
    }

    info!("CREATE DATASET: {}", dataset);

    let mut cmd = Command::new("/sbin/zfs");
    cmd.env_clear();
    cmd.arg("create");
    if parents {
        cmd.arg("-p");
    }
    for (k, v) in build_props(properties) {
        cmd.args(["-o", &format!("{}={}", k, v)])
    }
    cmd.arg(dataset);

    let zfs = cmd.output()?;

    if !zfs.status.success() {
        let errmsg = String::from_utf8_lossy(&zfs.stderr);
        bail!("zfs create failed: {}", errmsg);
    }

    Ok(())
}
