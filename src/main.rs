// main.rs

// *************************************************************************
// * Copyright (C) 2019 Arne Janbu (ajanbu@gmx.de)              *
// *                                                                       *
// * This program is free software: you can redistribute it and/or modify  *
// * it under the terms of the GNU General Public License as published by  *
// * the Free Software Foundation, either version 3 of the License, or     *
// * (at your option) any later version.                                   *
// *                                                                       *
// * This program is distributed in the hope that it will be useful,       *
// * but WITHOUT ANY WARRANTY; without even the implied warranty of        *
// * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the         *
// * GNU General Public License for more details.                          *
// *                                                                       *
// * You should have received a copy of the GNU General Public License     *
// * along with this program.  If not, see <http://www.gnu.org/licenses/>. *
// *************************************************************************

use std::ffi::OsString;
use std::fmt::Display;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use clap::Parser;
use directories::ProjectDirs;
use git2::Repository;
use raur::Raur;
use termion::{color, style};
use tokio::task;

#[derive(Debug, Parser)]
#[command(name = "taur", about = "Tiny AUR helper")]
struct Args {
    /// Local repo storage path (defaults to $HOME/.local/share/taur/repos)
    #[arg()]
    repos: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Parser)]
enum Command {
    /// Clone a repository from AUR
    #[command(name = "clone")]
    Clone { package_name: String },
    /// Fetch and print new commits for all repositories
    #[command(name = "fetch")]
    Fetch,
    /// Search for packages in AUR
    #[command(name = "search")]
    Search { expression: String },
    /// Pull given package repositories (if no package is specified, all repositories are pulled)
    #[command(name = "pull")]
    Pull { package_names: Vec<String> },
}

#[derive(Eq)]
struct UpdateInfo {
    name: String,
    commits: Vec<String>,
}

impl Display for UpdateInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(
            f,
            "{}{}:: {}{}{}",
            style::Bold,
            color::Fg(color::Blue),
            color::Fg(color::Reset),
            self.name,
            style::Reset
        )?;
        writeln!(f)?;

        for commit in &self.commits {
            writeln!(
                f,
                "{}* {}{}{}",
                color::Fg(color::Magenta),
                color::Fg(color::Cyan),
                commit,
                style::Reset
            )?;
        }

        Ok(())
    }
}

impl Ord for UpdateInfo {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name.cmp(&other.name)
    }
}

impl PartialEq for UpdateInfo {
    fn eq(&self, other: &UpdateInfo) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for UpdateInfo {
    fn partial_cmp(&self, other: &UpdateInfo) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    let proj_dirs =
        ProjectDirs::from("", "", "taur").expect("Unable to retrieve application directories");

    match &args.command {
        Some(cmd) => match cmd {
            Command::Clone { package_name } => {
                if let Err(e) = clone(proj_dirs, args.repos, package_name).await {
                    eprintln!("Error while cloning: {}", e);
                }
            }
            Command::Fetch => {
                if let Err(e) = fetch(proj_dirs, args.repos).await {
                    eprintln!("Error while fetching: {}", e);
                }
            }
            Command::Pull { package_names } => {
                if let Err(e) = pull(proj_dirs, args.repos, package_names).await {
                    eprintln!("Error while pulling: {}", e);
                }
            }
            Command::Search { expression } => {
                if let Err(e) = search(expression).await {
                    eprintln!("Error while searching: {}", e);
                }
            }
        },
        None => {
            if let Err(e) = fetch(proj_dirs, args.repos).await {
                eprintln!("Error while fetching: {}", e);
            }
        }
    }
}

async fn clone(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
    package_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let raur = raur::Handle::new();
    let pkgs = raur.info(&[package_name]).await?;

    if pkgs.is_empty() {
        return Err(Box::new(Error::new(
            ErrorKind::NotFound,
            format!("Package '{}' not found", package_name),
        )));
    }

    let repo_path = get_repo_path(proj_dirs, repos);
    if !repo_path.exists() {
        std::fs::create_dir_all(repo_path.as_ref())?;
    }

    let repo_path = repo_path.join(package_name);

    let url = format!("https://aur.archlinux.org/{}.git", package_name);

    match Repository::clone(&url, &repo_path) {
        Ok(_) => println!("Cloned repo '{}' to '{:?}'", package_name, repo_path),
        Err(e) => {
            return Err(Box::new(Error::new(
                ErrorKind::Other,
                format!("Error while cloning repo '{}': {}", package_name, e),
            )))
        }
    };

    Ok(())
}

async fn fetch(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = get_repo_path(proj_dirs, repos);
    if !repo_path.exists() {
        std::fs::create_dir_all(repo_path.as_ref())?;
    }

    let dirs = get_dir_list(&repo_path)?;

    let mut update_infos: Vec<UpdateInfo> = Vec::new();

    let (tx, rx) = mpsc::channel();
    let mut join_handles = vec![];

    for dir in dirs {
        let tx = mpsc::Sender::clone(&tx);
        let path_base = repo_path.clone();
        join_handles.push(task::spawn_blocking(move || {
            let full_path = path_base.join(dir);
            match check_repo_updates(full_path) {
                Ok(update_info) => {
                    if let Some(update_info) = update_info {
                        if let Err(e) = tx.send(update_info) {
                            eprintln!("Error while sending update info for printing: {}", e);
                        }
                    }
                }
                Err(e) => eprintln!("Error while checking for updates for repo {:?}", e),
            }
        }));
    }

    // Drop tx to get rid of the original unused sender
    drop(tx);

    futures::future::join_all(join_handles).await;

    for received in rx {
        update_infos.push(received);
    }

    print_update_info(update_infos);

    Ok(())
}

async fn search(expression: &str) -> Result<(), Box<dyn std::error::Error>> {
    let raur = raur::Handle::new();

    let mut pkgs = raur.search(expression).await?;
    pkgs.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    if pkgs.len() == 0 {
        println!("No packages found");
        return Ok(());
    }

    let longest_len = pkgs
        .iter()
        .max_by_key(|p| p.name.len())
        .map(|p| p.name.len())
        .unwrap_or_default();

    println!(
        "{}Pop  - Name{}Description{}",
        style::Bold,
        " ".repeat(std::cmp::max(longest_len - 3, 0)),
        style::Reset
    );

    for pkg in pkgs {
        println!(
            "{:.2} - {}{}{}{}{}",
            pkg.popularity,
            color::Fg(color::Magenta),
            pkg.name,
            style::Reset,
            " ".repeat(std::cmp::max(longest_len - pkg.name.len() + 1, 0)),
            pkg.description.unwrap_or_default()
        );
    }

    Ok(())
}

async fn pull(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
    package_names: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = get_repo_path(proj_dirs, repos);
    if !repo_path.exists() {
        std::fs::create_dir_all(repo_path.as_ref())?;
    }

    let mut join_handles = vec![];

    for package_name in package_names {
        let package_name = package_name.clone();
        let path_base = repo_path.clone();
        join_handles.push(task::spawn_blocking(move || {
            if let Err(e) = pull_package(&path_base, &package_name) {
                eprintln!("Error while pulling package: {:?}", e);
            }
        }));
    }

    futures::future::join_all(join_handles).await;

    Ok(())
}

fn pull_package(repo_path: &Path, package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let full_path = repo_path.join(package_name);

    let repo = Repository::open(&full_path)?;

    let update_info = check_repo_updates(full_path)?;

    match update_info {
        Some(update_info) => {
            println!("{}Pulling {}...{}", style::Bold, package_name, style::Reset);
            println!();
            for commit in update_info.commits {
                println!(
                    "{}* {}{}{}",
                    color::Fg(color::Magenta),
                    color::Fg(color::Cyan),
                    commit,
                    style::Reset
                );
            }
        }
        None => {
            println!("No new commits to pull");
            return Ok(());
        }
    }

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    let mut refs_heads_master = repo.find_reference("refs/heads/master")?;

    let name = match refs_heads_master.name() {
        Some(name) => name.to_string(),
        None => String::from_utf8_lossy(refs_heads_master.name_bytes()).to_string(),
    };

    let msg = format!(
        "Fast-Forward: Setting {} to id: {}",
        name,
        fetch_commit.id()
    );
    refs_heads_master.set_target(fetch_commit.id(), &msg)?;

    repo.set_head(&name)?;

    let checkout = &mut git2::build::CheckoutBuilder::default();
    checkout.force();
    repo.checkout_head(Some(checkout))?;

    Ok(())
}

fn get_repo_path(proj_dirs: ProjectDirs, repos: Option<PathBuf>) -> Box<PathBuf> {
    match repos {
        Some(s) => Box::new(s),
        None => Box::new(proj_dirs.data_dir().join("repos")),
    }
}

fn print_update_info(mut update_infos: Vec<UpdateInfo>) {
    if !update_infos.is_empty() {
        println!(
            "{}The following packages have upstream changes:{}",
            style::Bold,
            style::Reset
        );
        println!();

        update_infos.sort_unstable();

        for info in update_infos {
            println!("{}", info);
        }
    } else {
        println!("There are currently no packages with upstream changes");
    }
}

fn check_repo_updates(path: PathBuf) -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    let dir_name = path.file_name().ok_or("File name was None?!")?;
    let dir_name = String::from(dir_name.to_string_lossy());

    let repo = Repository::open(path)?;
    let mut remote = repo.find_remote("origin")?;
    remote.fetch(&["master"], None, None)?;

    let local_rev = repo.revparse_single("HEAD")?;
    let remote_rev = repo.revparse_single("@{u}")?;

    if local_rev.id() != remote_rev.id() {
        let mut revwalk = repo.revwalk()?;

        revwalk.push(remote_rev.id())?;
        revwalk.hide(local_rev.id())?;

        // println!("Local: {}", local_rev.id());
        // println!("Remote: {}", remote_rev.id());

        let mut commits: Vec<String> = Vec::new();

        for oid in revwalk {
            let oid = oid?;
            let commit = repo.find_commit(oid)?;
            if let Some(c) = commit.message() {
                commits.push(String::from(c));
            }
        }

        return Ok(Some(UpdateInfo {
            name: dir_name,
            commits,
        }));
    }

    Ok(None)
}

fn get_dir_list(pathbuf: &Path) -> Result<Vec<OsString>, Error> {
    let path = Path::new(pathbuf);
    let path_iter = std::fs::read_dir(path)?;

    let res = path_iter
        .map(|entry| match entry {
            Ok(e) => {
                let entry_path = e.path();
                if entry_path.is_dir() {
                    match entry_path.file_name() {
                        Some(f) => f.to_os_string(),
                        None => OsString::default(),
                    }
                } else {
                    OsString::default()
                }
            }
            Err(_) => OsString::default(),
        })
        .filter(|s| !s.is_empty())
        .collect::<Vec<OsString>>();

    Ok(res)
}
