use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};
use ultratasks_core::*;

fn save(store: &mut Store, title: &str) -> Task {
    let saved = store
        .save_task(Save::Create {
            id: TaskId::new().unwrap(),
            data: TaskData {
                title: Title::new(title).unwrap(),
                list: store.info().default_list,
                status: TaskStatus::Open,
                due: None,
            },
        })
        .unwrap();
    assert!(saved.operation.is_some());
    assert_eq!(
        store.get(EntityId::Task(saved.value.key.id)).unwrap(),
        Entity::Task(saved.value.clone())
    );
    saved.value
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

fn assert_multiple_links(path: &Path) {
    let Err(error) = Store::open(path) else {
        panic!("hardlink付きDBを開いてしまった: {path:?}");
    };
    assert!(matches!(
        error,
        Error::Storage {
            stage: StorageStage::Open,
            kind: StorageKind::MultipleLinks,
            outcome: CommitKnowledge::NotCommitted,
        }
    ));
    assert!(error.to_string().contains("ハードリンク"));
}

fn assert_lock_released(path: &Path) {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(sidecar(path, ".lock"))
        .unwrap();
    lock.try_lock().expect("失敗したopenのロックが残っている");
}

fn child(path: &Path, expected: &str) {
    let output = Command::new(std::env::current_exe().unwrap())
        .args(["--ignored", "--exact", "open_probe_child", "--nocapture"])
        .env("ULTRATASKS_IDENTITY_PATH", path)
        .env("ULTRATASKS_IDENTITY_EXPECT", expected)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "子プロセスの確認失敗: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("IDENTITY_CHILD_OK"));
}

#[test]
#[ignore = "親テストがDBを準備して子プロセスとして呼ぶ入口"]
fn open_probe_child() {
    let path = std::env::var_os("ULTRATASKS_IDENTITY_PATH").unwrap();
    match std::env::var("ULTRATASKS_IDENTITY_EXPECT")
        .unwrap()
        .as_str()
    {
        "locked" => assert!(matches!(
            Store::open(Path::new(&path)),
            Err(Error::AlreadyOpen)
        )),
        "hardlink" => {
            assert_multiple_links(Path::new(&path));
            assert_lock_released(Path::new(&path));
        }
        other => panic!("未知の子プロセス条件: {other}"),
    }
    println!("IDENTITY_CHILD_OK");
}

#[cfg(unix)]
fn directory_alias(original: &Path, alias: &Path) {
    std::os::unix::fs::symlink(original, alias).unwrap();
    println!("DIRECTORY_ALIAS: symlink");
}

#[cfg(windows)]
fn directory_alias(original: &Path, alias: &Path) {
    let output = Command::new("cmd.exe")
        .args(["/d", "/c", "mklink", "/J"])
        .arg(alias)
        .arg(original)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "junction作成失敗: {}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    println!("DIRECTORY_ALIAS: junction");
}

#[test]
#[cfg(any(windows, unix))]
fn new_database_normalized_paths_and_child_share_lock_then_restore() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new").join("nested").join("tasks.sqlite");
    assert!(!path.exists());
    let mut store = Store::open(&path).unwrap();
    let saved = save(&mut store, "新規保存と復元");
    let database_id = store.info().database;
    let history = store.history().unwrap();
    let linked_dir = dir.path().join("directory-alias");
    directory_alias(path.parent().unwrap(), &linked_dir);
    let alias = linked_dir.join("tasks.sqlite");
    assert_eq!(alias.canonicalize().unwrap(), path.canonicalize().unwrap());
    let dot = path.parent().unwrap().join(".").join("tasks.sqlite");
    for name in [&path, &dot, &alias] {
        assert!(matches!(Store::open(name), Err(Error::AlreadyOpen)));
        child(name, "locked");
    }
    drop(store);
    let mut reopened = Store::open(&alias).unwrap();
    assert_eq!(reopened.info().database, database_id);
    assert_eq!(reopened.history().unwrap(), history);
    assert_eq!(
        reopened.get(EntityId::Task(saved.key.id)).unwrap(),
        Entity::Task(saved.clone())
    );
    assert!(matches!(Store::open(&path), Err(Error::AlreadyOpen)));
    drop(reopened);
    assert_lock_released(&path);
    assert_eq!(
        Store::open(&path)
            .unwrap()
            .get(EntityId::Task(saved.key.id))
            .unwrap(),
        Entity::Task(saved)
    );
}

#[test]
#[cfg(any(windows, unix))]
fn file_symlink_uses_target_lock_and_wal_for_new_and_existing_database() {
    for existing in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("target.sqlite");
        let alias = dir.path().join("symlink.sqlite");
        if existing {
            drop(Store::open(&original).unwrap());
        }
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&original, &alias).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&original, &alias).unwrap();
        let mut store = Store::open(&alias).unwrap();
        let saved = save(&mut store, "symlink経由の保存");
        assert!(sidecar(&original, ".lock").exists());
        assert!(sidecar(&original, "-wal").exists());
        assert!(!sidecar(&alias, ".lock").exists());
        assert!(!sidecar(&alias, "-wal").exists());
        assert!(!sidecar(&alias, "-shm").exists());
        for name in [&original, &alias] {
            assert!(matches!(Store::open(name), Err(Error::AlreadyOpen)));
            child(name, "locked");
        }
        drop(store);
        assert_lock_released(&original);
        assert_eq!(
            Store::open(&original)
                .unwrap()
                .get(EntityId::Task(saved.key.id))
                .unwrap(),
            Entity::Task(saved)
        );
    }
}

#[test]
fn hardlink_names_and_processes_are_rejected_without_changing_saved_data() {
    for keep_alias in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.sqlite");
        let aliases = dir.path().join("aliases");
        fs::create_dir(&aliases).unwrap();
        let alias = aliases.join("alias.sqlite");
        let mut store = Store::open(&original).unwrap();
        let saved = save(&mut store, "hardlink作成前に確定した内容");
        let history = store.history().unwrap();
        let database_id = store.info().database;
        drop(store);
        let bytes = fs::read(&original).unwrap();
        fs::hard_link(&original, &alias).unwrap();
        assert_ne!(
            original.canonicalize().unwrap(),
            alias.canonicalize().unwrap()
        );
        for path in [&original, &alias] {
            assert_multiple_links(path);
            assert_lock_released(path);
            child(path, "hardlink");
            assert_eq!(fs::read(path).unwrap(), bytes);
            assert!(!sidecar(path, "-wal").exists());
            assert!(!sidecar(path, "-shm").exists());
        }
        let (removed, surviving) = if keep_alias {
            (&original, &alias)
        } else {
            (&alias, &original)
        };
        fs::remove_file(removed).unwrap();
        let mut reopened = Store::open(surviving).unwrap();
        assert_eq!(reopened.info().database, database_id);
        assert_eq!(reopened.history().unwrap(), history);
        assert_eq!(
            reopened.get(EntityId::Task(saved.key.id)).unwrap(),
            Entity::Task(saved)
        );
        save(&mut reopened, "リンクを一つに戻した後の保存");
    }
}

#[test]
fn empty_hardlinked_database_is_rejected_before_sqlite_initialization() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("empty.sqlite");
    let alias = dir.path().join("alias.sqlite");
    fs::write(&original, []).unwrap();
    fs::hard_link(&original, &alias).unwrap();
    for path in [&original, &alias] {
        assert_multiple_links(path);
        assert_eq!(fs::metadata(path).unwrap().len(), 0);
        assert_lock_released(path);
        assert!(!sidecar(path, "-wal").exists());
        assert!(!sidecar(path, "-shm").exists());
    }
    fs::remove_file(&alias).unwrap();
    save(&mut Store::open(&original).unwrap(), "初期化の再試行");
}

#[test]
fn link_added_during_open_cannot_start_another_wal() {
    let dir = tempfile::tempdir().unwrap();
    let original = dir.path().join("live.sqlite");
    let alias = dir.path().join("alias.sqlite");
    let mut store = Store::open(&original).unwrap();
    let saved = save(&mut store, "open中のデータ");
    fs::hard_link(&original, &alias).unwrap();
    assert_multiple_links(&alias);
    child(&alias, "hardlink");
    assert!(!sidecar(&alias, "-wal").exists());
    assert!(!sidecar(&alias, "-shm").exists());
    assert_eq!(
        store.get(EntityId::Task(saved.key.id)).unwrap(),
        Entity::Task(saved.clone())
    );
    drop(store);
    fs::remove_file(&alias).unwrap();
    assert_eq!(
        Store::open(&original)
            .unwrap()
            .get(EntityId::Task(saved.key.id))
            .unwrap(),
        Entity::Task(saved)
    );
}

#[test]
fn schema_error_after_lock_acquisition_releases_handles_and_preserves_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("future.sqlite");
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=2;").unwrap();
    drop(connection);
    let bytes = fs::read(&path).unwrap();
    assert!(matches!(
        Store::open(&path),
        Err(Error::UnsupportedSchema {
            found: 2,
            supported: 1
        })
    ));
    assert_lock_released(&path);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    let connection = rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("PRAGMA user_version=0;").unwrap();
    drop(connection);
    save(&mut Store::open(&path).unwrap(), "open失敗後の復元");
}
