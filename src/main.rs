use std::{fs, path::PathBuf, process::Stdio, time::Duration};

use actix_web::{App, HttpResponse, HttpServer, Responder, get, post, web};
use nix::{
    sys::{signal, wait::WaitStatus},
    unistd::{ForkResult, Pid},
};
use serde::{Deserialize, Serialize};
use signal_hook::{
    consts::signal::{SIGCHLD, SIGINT, SIGTERM},
    iterator::Signals,
};
use tmpdir::TmpDir;
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    runtime::Runtime,
    task, time,
};

mod tmpdir;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Lang {
    Clojure,
    Cpp,
    Csharp,
    Dart,
    Elixir,
    Golang,
    Haskell,
    Java,
    Js,
    Kotlin,
    Php,
    Python,
    Ruby,
    Rust,
    Swift,
    Ts,
}

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    timeout: Option<String>,
    solution_text: String,
    lang_slug: Lang,
    asserts: Option<String>,
    checker_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

fn ashy_slashy(child: Pid, mut sig: Signals) {
    for s in sig.forever() {
        if s == SIGINT || s == SIGTERM {
            log::info!("Caught signal, terminating child");
            std::thread::spawn(move || {
                if s == SIGINT {
                    let _ = signal::kill(child, Some(signal::SIGINT));
                    std::thread::sleep(Duration::from_secs(2));
                }
                let _ = signal::kill(child, Some(signal::SIGTERM));
                std::thread::sleep(Duration::from_secs(2));

                log::info!("Child process does not respond, killing it");
                let _ = signal::kill(child, Some(signal::SIGKILL));
            });
        }
        let status = match nix::sys::wait::wait() {
            Err(e) => {
                log::error!("Could not wait for child process: {}", e);
                std::process::exit(1);
            }
            Ok(status) => match status {
                WaitStatus::Exited(pid, status) => Some((pid, status)),
                WaitStatus::Signaled(pid, sig, _) => Some((pid, sig as i32 + 128)),
                _ => None,
            },
        };
        if let Some((pid, status)) = status {
            if pid == child {
                log::info!("Child exited, shutting down");
                std::process::exit(status);
            }
            log::debug!("Reaped zombie with pid {}. Groovy!", pid);
        }
    }
    log::error!("Signal stream exhausted, exiting");
    std::process::exit(1);
}

async fn read_stdio<R: AsyncRead + Unpin>(mut reader: R) -> std::io::Result<String> {
    let mut buf = String::new();
    reader.read_to_string(&mut buf).await?;
    Ok(buf)
}

fn get_solution_chekcer_names(payload: &Payload) -> (&str, Option<&str>) {
    match payload.lang_slug {
        Lang::Clojure => ("solution.clj", None),
        Lang::Cpp => ("solution.cpp", Some("checker.cpp")),
        Lang::Csharp => ("Solution.cs", Some("Checker.cs")),
        Lang::Dart => ("solution.dart", Some("checker.dart")),
        Lang::Elixir => ("solution.exs", None),
        Lang::Golang => ("solution.go", Some("checker.go")),
        Lang::Haskell => ("Solution.hs", Some("Checker.hs")),
        Lang::Java => ("Solution.java", Some("Checker.java")),
        Lang::Js | Lang::Ts => ("solution.js", None),
        // Lang::Ts => ("solution.ts", None),
        Lang::Kotlin => ("solution.kt", Some("checker.kt")),
        Lang::Php => ("solution.php", None),
        Lang::Python => ("solution.py", None),
        Lang::Ruby => ("solution.rb", None),
        Lang::Rust => ("solution.rs", Some("checker.rs")),
        Lang::Swift => ("solution.swift", Some("checker.swift")),
    }
}

#[post("/run")]
async fn run(
    web::Json(payload): web::Json<Payload>,
) -> Result<web::Json<Response>, actix_web::Error> {
    log::debug!("{}", serde_json::to_string(&payload).unwrap());
    let timeout = match payload.timeout {
        Some(ref t) => duration_str::parse(t).map_err(|e| {
            log::error!("Parse timeout: {}", e);
            actix_web::error::ErrorBadRequest("wrong timeout format")
        })?,
        None => DEFAULT_TIMEOUT,
    };

    if matches!(
        payload.lang_slug,
        Lang::Cpp
            | Lang::Csharp
            | Lang::Dart
            | Lang::Java
            | Lang::Golang
            | Lang::Haskell
            | Lang::Kotlin
            | Lang::Rust
            | Lang::Swift,
    ) && payload.checker_text.is_none()
    {
        return Err(actix_web::error::ErrorBadRequest(
            "checker_text is required",
        ));
    }

    let tmp = TmpDir::new().map_err(|e| {
        log::error!("Create tmp dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    let cwd = std::env::current_dir().map_err(|e| {
        log::error!("Get current dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    let check_dir = if matches!(payload.lang_slug, Lang::Dart) {
        "lib"
    } else {
        "check"
    };

    let check_path = tmp
        .chroot()
        .join(PathBuf::from_iter(cwd.components().skip(1))) // Drop the root component for correct join
        .join(check_dir);

    log::debug!("Check path is: {}", check_path.display());

    if let Some(ref asserts) = payload.asserts {
        fs::write(check_path.join("asserts.json"), asserts.as_bytes()).map_err(|e| {
            log::error!("Write asserts file: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;
    }

    let (solution_filename, checker_filename) = get_solution_chekcer_names(&payload);

    fs::write(
        check_path.join(solution_filename),
        payload.solution_text.as_bytes(),
    )
    .map_err(|e| {
        log::error!("Write solution file: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    if let Some(checker_filename) = checker_filename {
        fs::write(
            check_path.join(checker_filename),
            payload.checker_text.as_ref().unwrap(),
        )
        .map_err(|e| {
            log::error!("Write checker text: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;
    }

    let mut cmd = Command::new("make");
    unsafe {
        let chroot_path = tmp.chroot().clone();
        cmd.arg("--silent")
            .arg("test")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .pre_exec(move || {
                // Only Linux has namespaces feature and this code is supposed
                // to work only in container environment.
                // Condition here exists only for the purpose of muting errors
                // on other systems during development
                #[cfg(target_os = "linux")]
                {
                    use nix::sched::CloneFlags;
                    // Create new namespaces for isolation
                    nix::sched::unshare(
                        CloneFlags::CLONE_FS
                            | CloneFlags::CLONE_FILES
                            | CloneFlags::CLONE_NEWNS
                            // | CloneFlags::CLONE_NEWUSER TODO swift doesn't work with this
                            // | CloneFlags::CLONE_NEWPID TODO figure out how to use that properly
                            | CloneFlags::CLONE_NEWNET,
                    )?;
                }
                // Chroot to put current execution in jail
                nix::unistd::chroot(&chroot_path)?;
                std::env::set_current_dir(&cwd).unwrap();
                Ok(())
            });
    }
    let mut child = cmd.spawn().unwrap();
    let stdout_handle = task::spawn(read_stdio(child.stdout.take().unwrap()));
    let stderr_handle = task::spawn(read_stdio(child.stderr.take().unwrap()));

    let exit_code = match time::timeout(timeout, child.wait()).await {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Timeout: {}", e);
            let pid = child.id().unwrap() as i32;
            if let Err(e) = signal::killpg(Pid::from_raw(pid), signal::SIGKILL) {
                log::error!("Cannot kill child group: {}", e);
                std::process::exit(1);
            }
            return Err(actix_web::error::ErrorRequestTimeout("timelimit exceeded"));
        }
    }
    .map_err(|e| {
        log::error!("Run check: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    let stdout = stdout_handle.await.map_err(|e| {
        log::error!("Join stdout task: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })??;
    let stderr = stderr_handle.await.map_err(|e| {
        log::error!("Join stdout task: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })??;

    log::debug!("STDOUT: {}", stdout);
    log::debug!("STDERR: {}", stderr);
    Ok(web::Json(Response {
        exit_code: exit_code.code(),
        stdout,
        stderr,
    }))
}

#[get("/health")]
async fn health() -> impl Responder {
    HttpResponse::Ok()
}

fn main() -> anyhow::Result<()> {
    env_logger::init();
    log::info!("Runner version {}", VERSION);

    if std::process::id() == 1 {
        log::info!(
            "We're the init process, forking and calling Ashy Slashy to take care of 'em zombies"
        );
        unsafe {
            if let ForkResult::Parent { child } = nix::unistd::fork()? {
                let signals = Signals::new([SIGCHLD, SIGINT, SIGTERM])?;
                ashy_slashy(child, signals);
            }
        }
    };

    let rt = Runtime::new()?;
    rt.block_on(async {
        log::info!("Starting runner service");
        HttpServer::new(|| {
            let json_config = web::JsonConfig::default().limit(10485760);
            App::new()
                .app_data(json_config)
                .service(run)
                .service(health)
        })
        .bind(("0.0.0.0", 8000))?
        .run()
        .await?;
        log::info!("Service stopped");
        Ok(())
    })
}
