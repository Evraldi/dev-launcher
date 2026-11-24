// Integration test terpisah: memakai API publik library tanpa menyalakan GUI.
use dev_launcher_lib::models::{Profile, Step};
use dev_launcher_lib::{journal, runner, storage};
use std::path::PathBuf;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dev-launcher-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn journal_add_load_and_remove_pid() {
    let base = temp_dir("journal");
    storage::init(base.clone()).unwrap();

    journal::add("it-profile", 0, 4_242_424, 0);
    assert!(
        journal::load()
            .entries
            .iter()
            .any(|e| e.pid == 4_242_424 && e.profile_id == "it-profile"),
        "PID harus tercatat di journal"
    );

    journal::remove_pid("it-profile", 4_242_424);
    assert!(
        !journal::load().entries.iter().any(|e| e.pid == 4_242_424),
        "PID harus hilang dari journal"
    );

    let _ = std::fs::remove_dir_all(base);
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::net::TcpListener;
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    fn silent(cmd: &mut Command) {
        cmd.creation_flags(dev_launcher_lib::util::CREATE_NO_WINDOW);
    }

    #[test]
    fn spawned_process_exit_code_is_preserved() {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "exit 7"]);
        silent(&mut cmd);

        let status = cmd.status().unwrap();
        assert_eq!(status.code(), Some(7));
    }

    #[test]
    fn kill_tree_terminates_only_the_spawned_process() {
        let mut cmd = Command::new("cmd");
        cmd.args(["/C", "ping -n 30 127.0.0.1 >nul"]);
        silent(&mut cmd);

        let mut child = cmd.spawn().unwrap();
        let pid = child.id();
        assert!(
            runner::alive_pids(&[pid]).contains(&pid),
            "proses yang baru di-spawn harus terdeteksi hidup"
        );

        runner::kill_tree(pid);
        let _ = child.wait();
        assert!(
            runner::alive_pids(&[pid]).is_empty(),
            "proses harus mati setelah kill_tree"
        );
    }

    #[test]
    fn preflight_detects_occupied_port() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();

        let profile = Profile {
            id: "it-port".into(),
            name: "Integration Port".into(),
            folder: None,
            order: 0,
            external: false,
            steps: vec![Step::Command {
                cwd: ".".into(),
                cmd: "echo hi".into(),
                port: Some(port),
                shell: None,
                env: None,
                readiness: None,
                ready_timeout_ms: None,
                delay_ms: None,
                optional: None,
                auto_restart: None,
                max_restarts: None,
            }],
        };

        let result = runner::preflight(&profile).unwrap();
        assert!(
            result.conflicts.iter().any(|c| c.port == port),
            "port yang sedang di-bind harus dilaporkan bentrok: {result:?}"
        );

        drop(listener);
    }

    #[test]
    fn kill_port_owner_rejects_non_owner() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        // PID yang jelas-jelas bukan pemilik: jangan sampai taskkill jalan.
        let bogus_pid = std::process::id().saturating_add(100_000);

        let err = runner::kill_port_owner(bogus_pid, port).unwrap_err();
        assert!(err.contains("tidak memegang"), "{err}");

        drop(listener);
    }
}
