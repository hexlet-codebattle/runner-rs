use std::{fs, os::unix, path::PathBuf, process::Stdio};

use actix_web::{http::StatusCode, post, web, App, HttpServer};
use serde::{Deserialize, Serialize};
use temp_dir::TempDir;
use tokio::process::Command;

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
}

#[derive(Debug, Serialize, Deserialize)]
struct Payload {
    timeout: String,
    solution_text: String,
    lang_slug: Lang,
    asserts: String,
    checker_text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Response {
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
}

macro_rules! internal_error {
    () => {
        actix_web::error::InternalError::new("internal error", StatusCode::INTERNAL_SERVER_ERROR)
    };
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
    let tmp = TempDir::new().map_err(|e| {
        log::error!("create tmp dir: {}", e);
        internal_error!()
    })?;
    let cwd = std::env::current_dir().map_err(|e| {
        log::error!("get current dir: {}", e);
        internal_error!()
    })?;
    let check_path = if matches!(payload.lang_slug, Lang::Dart) {
        tmp.path().join("lib")
    } else {
        tmp.path().join("check")
    };
    fs::create_dir(&check_path).map_err(|e| {
        log::error!("create check dir: {}", e);
        internal_error!()
    })?;
    unix::fs::symlink(cwd.join("Makefile"), tmp.path().join("Makefile")).map_err(|e| {
        log::error!("symlink makefile: {}", e);
        internal_error!()
    })?;
    fs::write(check_path.join("asserts.json"), payload.asserts.as_bytes()).map_err(|e| {
        log::error!("write asserts file: {}", e);
        internal_error!()
    })?;

    let solution_filename;
    match payload.lang_slug {
        Lang::Clojure => {
            solution_filename = "solution.clj";
            make_symlinks(&cwd, &tmp.path().to_owned(), ["runner.clj", "bb.edn"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                internal_error!()
            })?;
            fs::copy(cwd.join("checker.clj"), check_path.join("checker.clj")).map_err(|e| {
                log::error!("copy checker.clj: {}", e);
                internal_error!()
            })?;
        }
        Lang::Cpp => {
            solution_filename = "solution.cpp";
            fs::copy(cwd.join("checker.cpp"), check_path.join("checker.cpp")).map_err(|e| {
                log::error!("copy checker.cpp: {}", e);
                internal_error!()
            })?;
        }
        Lang::Csharp => {
            solution_filename = "Solution.cs";
            make_symlinks(&cwd, &tmp.path().to_owned(), ["Program.cs"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                internal_error!()
            })?;
            fs::copy(cwd.join("Checker.cs"), check_path.join("Checker.cs")).map_err(|e| {
                log::error!("copy Checker.cs: {}", e);
                internal_error!()
            })?;
        }
        Lang::Dart => {
            solution_filename = "solution.dart";
            make_symlinks(&cwd, &tmp.path().to_owned(), ["pubspec.yml"]).map_err(|e| {
                log::error!("symlink files: {}", e);
                internal_error!()
            })?;
            fs::copy(cwd.join("checker.dart"), check_path.join("checker.dart")).map_err(|e| {
                log::error!("copy checker.dart: {}", e);
                internal_error!()
            })?;
        }
        Lang::Elixir => {
            solution_filename = "solution.exs";
            make_symlinks(
                &cwd,
                &tmp.path().to_owned(),
                ["mix.exs", "mix.lock", "runner.exs"],
            )
            .map_err(|e| {
                log::error!("symlink files: {}", e);
                internal_error!()
            })?;
            fs::copy(cwd.join("checker.exs"), check_path.join("checker.exs")).map_err(|e| {
                log::error!("copy checker.exs: {}", e);
                internal_error!()
            })?;
        }
        Lang::Golang => {
            solution_filename = "solution.go";
            fs::copy(cwd.join("checker.go"), check_path.join("checker.go")).map_err(|e| {
                log::error!("copy checker.go: {}", e);
                internal_error!()
            })?;
        }
        Lang::Haskell => {
            //solution_filename = "Solution.hs";
            return Err(actix_web::error::InternalError::new(
                "unimplemented",
                StatusCode::BAD_REQUEST,
            )
            .into());
        }
        Lang::Java => {
            solution_filename = "Solution.java";
            make_symlinks(
                &cwd,
                &tmp.path().to_owned(),
                ["javax_json.jar", "javax_json_api.jar"],
            )
            .map_err(|e| {
                log::error!("symlink files: {}", e);
                internal_error!()
            })?;
            fs::copy(cwd.join("Checker.java"), check_path.join("Checker.java")).map_err(|e| {
                log::error!("copy Checker.java: {}", e);
                internal_error!()
            })?;
        }
        Lang::Js => {
            //solution_filename = "solution.js";
            return Err(actix_web::error::InternalError::new(
                "unimplemented",
                StatusCode::BAD_REQUEST,
            )
            .into());
        }
        Lang::Kotlin => {
            solution_filename = "solution.kt";
            fs::copy(cwd.join("checker.kt"), check_path.join("checker.kt")).map_err(|e| {
                log::error!("copy checker.kt: {}", e);
                internal_error!()
            })?;
        }
        Lang::Php => {
            solution_filename = "solution.php";
            fs::copy(cwd.join("checker.php"), check_path.join("checker.php")).map_err(|e| {
                log::error!("copy checker.php: {}", e);
                internal_error!()
            })?;
        }
        Lang::Python => {
            solution_filename = "solution.py";
            fs::copy(cwd.join("checker.py"), tmp.path().join("checker.py")).map_err(|e| {
                log::error!("copy checker.py: {}", e);
                internal_error!()
            })?;
        }
        Lang::Ruby => {
            solution_filename = "solution.rb";
            fs::copy(cwd.join("checker.rb"), tmp.path().join("checker.rb")).map_err(|e| {
                log::error!("copy checker.rb: {}", e);
                internal_error!()
            })?;
        }
    }

    fs::write(
        check_path.join(solution_filename),
        payload.solution_text.as_bytes(),
    )
    .map_err(|e| {
        log::error!("write solution file: {}", e);
        internal_error!()
    })?;

    let out = Command::new("make")
        .arg("-C")
        .arg(tmp.path())
        .arg("test")
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
        .wait_with_output()
        .await
        .map_err(|e| {
            log::error!("run check: {}", e);
            internal_error!()
        })?;
    let stdout = String::from_utf8(out.stdout).unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    if !out.status.success() {
        log::error!("run check stdout: {}", stdout,);
        //log::error!(
        //"run check stderr: {:?}",
        //String::from_utf8(out.stderr).unwrap()
        //);
        return Err(actix_web::error::InternalError::new(
            stdout,
            StatusCode::INTERNAL_SERVER_ERROR,
        )
        .into());
    };

    log::debug!("{}", stdout);
    Ok(web::Json(Response {
        exit_code: out.status.code(),
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
