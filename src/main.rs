use std::{fs, os::unix, path::PathBuf, process::Stdio, time::Duration};

use actix_web::{post, web, App, HttpServer};
use serde::{Deserialize, Serialize};
use tokio::{io::AsyncReadExt, process::Command, time};
use uuid::Uuid;

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
    Ts,
}

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    timeout: String,
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

#[post("/run")]
async fn run(payload: web::Json<Payload>) -> Result<web::Json<Response>, actix_web::Error> {
    log::debug!("{}", serde_json::to_string(&payload).unwrap());
    let timeout: u64 = payload.timeout.parse().map_err(|e| {
        log::error!("parse timeout: {}", e);
        actix_web::error::ErrorBadRequest("wrong timeout format")
    })?;

    if matches!(
        payload.lang_slug,
        Lang::Clojure
            | Lang::Cpp
            | Lang::Csharp
            | Lang::Dart
            | Lang::Golang
            | Lang::Haskell
            | Lang::Kotlin
            | Lang::Php
    ) {
        if payload.checker_text.is_none() {
            return Err(actix_web::error::ErrorBadRequest(
                "checker_text is required",
            ));
        }
    }

    let tmp = TmpDir::new().map_err(|e| {
        log::error!("create tmp dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    let tmp_path = tmp.path();

    let cwd = std::env::current_dir().map_err(|e| {
        log::error!("get current dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    let check_path = if matches!(payload.lang_slug, Lang::Dart) {
        tmp_path.join("lib")
    } else if matches!(payload.lang_slug, Lang::Haskell) {
        tmp_path.join("Check")
    } else {
        tmp_path.join("check")
    };
    fs::create_dir(&check_path).map_err(|e| {
        log::error!("create check dir: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;
    unix::fs::symlink(cwd.join("Makefile"), tmp_path.join("Makefile")).map_err(|e| {
        log::error!("symlink makefile: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    if let Some(ref asserts) = payload.asserts {
        fs::write(check_path.join("asserts.json"), asserts.as_bytes()).map_err(|e| {
            log::error!("write asserts file: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;
    }

    let solution_filename;
    match payload.lang_slug {
        Lang::Clojure => {
            solution_filename = "solution.clj";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["runner.clj", "bb.edn"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.clj"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write checker.clj: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Cpp => {
            solution_filename = "solution.cpp";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["json.hpp", "fifo_map.hpp"],
            )
            .map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.cpp"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write checker.cpp: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Csharp => {
            solution_filename = "Solution.cs";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["Program.cs"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.cs"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write Checker.cs: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Dart => {
            solution_filename = "solution.dart";
            make_symlinks(&cwd, &tmp_path.to_owned(), ["pubspec.yml"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("checker.dart"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write checker.dart: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Elixir => {
            solution_filename = "solution.exs";
            fs::copy(cwd.join("checker"), tmp_path.join("checker")).map_err(|e| {
                log::error!("copy checker: {}", e);
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
                log::error!("write checker.go: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Haskell => {
            solution_filename = "Solution.hs";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["HOwl.cabal", "magic.hs", "test_haskell.hs"],
            )
            .map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.hs"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write Checker.hs: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Java => {
            solution_filename = "Solution.java";
            make_symlinks(
                &cwd,
                &tmp_path.to_owned(),
                ["javax_json.jar", "javax_json_api.jar"],
            )
            .map_err(|e| {
                log::error!("symlink files: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
            fs::write(
                check_path.join("Checker.java"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write Checker.java: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Js => {
            solution_filename = "solution.js";
            fs::copy(cwd.join("checker.js"), tmp_path.join("checker.js")).map_err(|e| {
                log::error!("copy checker.js: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Ts => {
            solution_filename = "solution.ts";
            fs::copy(cwd.join("checker.js"), tmp_path.join("checker.js")).map_err(|e| {
                log::error!("copy checker.js: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Kotlin => {
            solution_filename = "solution.kt";
            fs::write(
                check_path.join("checker.kt"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write checker.kt: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Php => {
            solution_filename = "solution.php";
            fs::write(
                check_path.join("checker.php"),
                payload.checker_text.as_ref().unwrap(),
            )
            .map_err(|e| {
                log::error!("write checker.php: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Python => {
            solution_filename = "solution.py";
            fs::copy(cwd.join("checker.py"), tmp_path.join("checker.py")).map_err(|e| {
                log::error!("copy checker.py: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
        Lang::Ruby => {
            solution_filename = "solution.rb";
            fs::copy(cwd.join("checker.rb"), tmp_path.join("checker.rb")).map_err(|e| {
                log::error!("copy checker.rb: {}", e);
                actix_web::error::ErrorInternalServerError("internal error")
            })?;
        }
    }

    fs::write(
        check_path.join(solution_filename),
        payload.solution_text.as_bytes(),
    )
    .map_err(|e| {
        log::error!("write solution file: {}", e);
        actix_web::error::ErrorInternalServerError("internal error")
    })?;

    let mut child = Command::new("make")
        .arg("--silent")
        .arg("-C")
        .arg(tmp_path)
        .arg("test")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut child_stdout = child.stdout.take().unwrap();
    let mut child_stderr = child.stderr.take().unwrap();

    let exit_code = time::timeout(Duration::from_secs(timeout), child.wait())
        .await
        .map_err(|e| {
            log::warn!("timeout: {}", e);
            actix_web::error::ErrorRequestTimeout("timelimit exceeded")
        })?
        .map_err(|e| {
            log::error!("run check: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;

    let mut stdout = String::new();
    child_stdout
        .read_to_string(&mut stdout)
        .await
        .map_err(|e| {
            log::error!("read check stdout: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;
    let mut stderr = String::new();
    child_stderr
        .read_to_string(&mut stderr)
        .await
        .map_err(|e| {
            log::error!("read check stderr: {}", e);
            actix_web::error::ErrorInternalServerError("internal error")
        })?;

    log::debug!("{}", stdout);
    Ok(web::Json(Response {
        exit_code: exit_code.code(),
        stdout,
        stderr,
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();
    HttpServer::new(|| App::new().service(run))
        .bind(("0.0.0.0", 8000))?
        .run()
        .await?;
    log::debug!("DONE");
    Ok(())
}
