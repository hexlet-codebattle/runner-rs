use std::{
    fs,
    os::unix::{self, process::CommandExt},
    path::PathBuf,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use actix_web::{get, post, web, App, HttpResponse, HttpServer, Responder};
use nix::{
    sys::{signal, wait::WaitStatus},
    unistd::{ForkResult, Pid},
};
use serde::{Deserialize, Serialize};
use signal_hook::{
    consts::signal::{SIGCHLD, SIGINT, SIGTERM},
    iterator::Signals,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
    runtime::Runtime,
    task, time,
};
use uuid::Uuid;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
const VERSION: &str = env!("CARGO_PKG_VERSION");

struct TmpDir {
    path: PathBuf,
}

impl TmpDir {
    fn new() -> std::io::Result<Self> {
        let mut path = PathBuf::from(String::from("/tmp"));
        path.push(Uuid::new_v4().to_string());
        fs::create_dir(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &PathBuf {
        &self.path
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        if let Err(e) = fs::remove_dir_all(&self.path) {
            log::error!("remove tmp dir: {}", e);
        }
    }
}

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

fn make_symlinks<'a, I: IntoIterator<Item = &'a str>>(
    src: &PathBuf,
    dst: &PathBuf,
    files: I,
) -> anyhow::Result<()> {
    for f in files {
        unix::fs::symlink(src.join(f), dst.join(f))?;
    }
    Ok(())
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

#[post("/run")]
async fn run(payload: web::Json<Payload>) -> Result<web::Json<Response>, actix_web::Error> {
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
            | Lang::Rust,
    ) {
        if payload.checker_text.is_none() {
            return Err(actix_web::error::ErrorBadRequest(
                "checker_text is required",
            ));
        }
    }

    let tmp = TmpDir::new().map_err(|e| {
        log::error!("Create tmp dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    let tmp_path = tmp.path();

    let cwd = std::env::current_dir().map_err(|e| {
        log::error!("Get current dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    let check_path = if matches!(payload.lang_slug, Lang::Dart) {
        tmp_path.join("lib")
    } else {
        tmp_path.join("check")
    };
    fs::create_dir(&check_path).map_err(|e| {
        log::error!("Create check dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    unix::fs::symlink(cwd.join("Makefile"), tmp_path.join("Makefile")).map_err(|e| {
        log::error!("Symlink makefile: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    if let Some(ref asserts) = payload.asserts {
        fs::write(check_path.join("asserts.json"), asserts.as_bytes()).map_err(|e| {
            log::error!("Write asserts file: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;
    }

    let solution_filename;
    match payload.lang_slug {
        Lang::Clojure => {
            solution_filename = "solution.clj";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["checker.clj"]).map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Cpp => {
            solution_filename = "solution.cpp";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["json.hpp", "fifo_map.hpp", "check/checker.hpp.gch"],
            )
            .map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.cpp"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write checker.cpp: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Csharp => {
            solution_filename = "Solution.cs";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["Program.cs", "app.csproj", "obj"],
            )
            .map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.cs"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write Checker.cs: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Dart => {
            solution_filename = "solution.dart";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["pubspec.yml", "pubspec.lock", ".dart_tool"],
            )
            .map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.dart"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write checker.dart: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Elixir => {
            solution_filename = "solution.exs";
            fs::copy(cwd.join("checker"), tmp_path.join("checker")).map_err(|e| {
                log::error!("Copy checker: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Golang => {
            solution_filename = "solution.go";
            fs::write(
                check_path.join("checker.go"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write checker.go: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Haskell => {
            solution_filename = "Solution.hs";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["checker.cabal"]).map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.hs"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write Checker.hs: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Java => {
            solution_filename = "Solution.java";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["gson.jar"]).map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.java"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write Checker.java: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Js => {
            solution_filename = "solution.js";
            fs::copy(cwd.join("checker.js"), tmp_path.join("checker.js")).map_err(|e| {
                log::error!("Copy checker.js: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Ts => {
            solution_filename = "solution.ts";
            fs::copy(cwd.join("checker.js"), tmp_path.join("checker.js")).map_err(|e| {
                log::error!("Copy checker.js: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Kotlin => {
            solution_filename = "solution.kt";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["gson.jar"]).map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.kt"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write checker.kt: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Php => {
            solution_filename = "solution.php";
            fs::copy(cwd.join("checker.php"), tmp_path.join("checker.php")).map_err(|e| {
                log::error!("Copy checker.php: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Python => {
            solution_filename = "solution.py";
            fs::copy(cwd.join("checker.py"), tmp_path.join("checker.py")).map_err(|e| {
                log::error!("Copy checker.py: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Ruby => {
            solution_filename = "solution.rb";
            fs::copy(cwd.join("checker.rb"), tmp_path.join("checker.rb")).map_err(|e| {
                log::error!("Copy checker.rb: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Rust => {
            solution_filename = "solution.rs";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["Cargo.toml", "Cargo.lock", "target"],
            )
            .map_err(|e| {
                log::error!("Symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.rs"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("Write checker.rs: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
    }

    fs::write(
        check_path.join(solution_filename),
        payload.solution_text.as_bytes(),
    )
    .map_err(|e| {
        log::error!("Write solution file: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    let mut cmd = StdCommand::new("make");
    cmd.arg("--silent")
        .arg("-C")
        .arg(tmp_path)
        .arg("test")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = Command::from(cmd).spawn().unwrap();
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
            match nix::unistd::fork()? {
                ForkResult::Parent { child } => {
                    let signals = Signals::new(&[SIGCHLD, SIGINT, SIGTERM])?;
                    ashy_slashy(child, signals);
                }
                _ => {}
            }
        }
    };

    let rt = Runtime::new()?;
    rt.block_on(async {
        log::info!("Starting runner service");
        HttpServer::new(|| App::new().service(run).service(health))
            .bind(("0.0.0.0", 8000))?
            .run()
            .await?;
        log::info!("Service stopped");
        Ok(())
    })
}
