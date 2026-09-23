// Compiled as a native executable by the portable process-test harness.
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::thread;
use std::time::{Duration, Instant};

fn hold(root: &std::path::Path, stage: &str) {
    if env::var("DTR_FIXTURE_WAIT").ok().as_deref() == Some(stage) {
        fs::write(root.join("ready"), b"").unwrap();
        let deadline = Instant::now() + Duration::from_secs(30);
        while !root.join("release").exists() {
            assert!(Instant::now() < deadline, "fixture was not released");
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn main() {
    let root = PathBuf::from(env::var_os("DTR_FIXTURE").unwrap());
    let args: Vec<_> = env::args_os().skip(1).collect();
    let stage = args[0].to_str().unwrap();
    fs::write(root.join(format!("{stage}.args")), format!("{args:?}")).unwrap();
    fs::write(
        root.join(format!("{stage}.cwd")),
        env::current_dir().unwrap().as_os_str().as_encoded_bytes(),
    )
    .unwrap();
    fs::write(
        root.join(format!("{stage}.reexec")),
        env::var("DTR_SELF_REEXEC").unwrap_or_default(),
    )
    .unwrap();
    fs::write(
        root.join(format!("{stage}.locks")),
        env::var("DTR_SELF_LOCKS").unwrap_or_default(),
    )
    .unwrap();
    hold(&root, stage);
    match stage {
        "metadata" => print!(
            "{}",
            fs::read_to_string(root.join("metadata.json")).unwrap()
        ),
        "build" => {
            eprintln!("fixture build diagnostic");
            let code = env::var("DTR_FIXTURE_BUILD_EXIT")
                .unwrap_or_else(|_| "0".into())
                .parse()
                .unwrap();
            if code != 0 {
                process::exit(code);
            }
            print!(
                "{}",
                fs::read_to_string(root.join("artifact.json")).unwrap()
            );
        }
        "install" => {
            let mut code = env::var("DTR_FIXTURE_INSTALL_EXIT")
                .unwrap_or_else(|_| "0".into())
                .parse()
                .unwrap();
            if env::var_os("DTR_FIXTURE_FAIL_OTHER").is_some()
                && args
                    .iter()
                    .any(|arg| arg.to_string_lossy().ends_with("other-tool"))
            {
                code = 23;
            }
            if code == 0 && env::var_os("DTR_FIXTURE_NO_REPLACEMENT").is_none() {
                let original = PathBuf::from(env::var_os("DTR_FIXTURE_ORIGINAL").unwrap());
                // Models Cargo publishing a newly linked executable. Unlinking
                // first also works on Linux when the old image is mapped.
                if original.exists() {
                    fs::remove_file(&original).unwrap();
                }
                fs::copy(env::var_os("DTR_FIXTURE_REPLACEMENT").unwrap(), original).unwrap();
            }
            hold(&root, "published");
            process::exit(code);
        }
        _ => panic!("unexpected Cargo command: {args:?}"),
    }
}
