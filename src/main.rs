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

use directories::ProjectDirs;
use git2::Repository;
use std::ffi::OsString;
use std::fmt::Display;
use std::io::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use structopt::StructOpt;
use termion::{color, style};

#[derive(Debug, StructOpt)]
#[structopt(name = "taur", about = "Tiny AUR helper")]
struct Args {
    /// Local repo storage path (defaults to $HOME/.local/share/taur/repos)
    #[structopt()]
    repos: Option<PathBuf>,
    #[structopt(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// Clone a repository from AUR
    #[structopt(name = "clone")]
    Clone { package_name: String },
    /// Fetch and print new commits for all repositories
    #[structopt(name = "fetch")]
    Fetch,
    /// Search for packages in AUR
    #[structopt(name = "search")]
    Search { expression: String },
    /// Pull given package repositories (if no package is specified, all repositories are pulled)
    #[structopt(name = "pull")]
    Pull { package_names: Vec<String> },
}

#[derive(Eq, Ord)]
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

impl PartialEq for UpdateInfo {
    fn eq(&self, other: &UpdateInfo) -> bool {
        self.name == other.name
    }
}

impl PartialOrd for UpdateInfo {
    fn partial_cmp(&self, other: &UpdateInfo) -> Option<std::cmp::Ordering> {
        self.name.partial_cmp(&other.name)
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::from_args();

    let proj_dirs =
        ProjectDirs::from("", "", "taur").expect("Unable to retrieve application directories");

    match &args.command {
        Some(cmd) => match cmd {
            Command::Clone { package_name } => clone(proj_dirs, args.repos, package_name)?,
            Command::Fetch => fetch(proj_dirs, args.repos)?,
            Command::Pull { package_names } => pull(proj_dirs, args.repos, package_names)?,
            Command::Search { expression } => search(expression)?,
        },
        None => fetch(proj_dirs, args.repos)?,
    }

    Ok(())
}

fn clone(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
    package_name: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = get_repo_path(proj_dirs, repos)?;
    if !repo_path.exists() {
        std::fs::create_dir_all(repo_path.as_ref())?;
    }

    let repo_path = repo_path.join(package_name);

    let mut url = String::from("https://aur.archlinux.org/");
    url.push_str(package_name);
    url.push_str(".git");

    match Repository::clone(&url, &repo_path) {
        Ok(_) => println!("Cloned repo '{}' to '{:?}'", package_name, repo_path),
        Err(e) => eprintln!("Error while cloning repo '{}': {}", package_name, e),
    };

    Ok(())
}

fn fetch(proj_dirs: ProjectDirs, repos: Option<PathBuf>) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = get_repo_path(proj_dirs, repos)?;
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
        join_handles.push(thread::spawn(move || {
            match check_repo_updates(dir, path_base.to_path_buf(), tx) {
                Ok(_) => {}
                Err(e) => eprintln!("Error while checking for updates for repo {:?}", e),
            }
        }));
    }

    // Drop tx to get rid of the original unused sender
    drop(tx);

    for join_handle in join_handles {
        match join_handle.join() {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to join thread: {:?}", e),
        };
    }

    for received in rx {
        update_infos.push(received);
    }

    print_update_info(update_infos);

    Ok(())
}

fn search(expression: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut pkgs = raur::search(expression)?;
    pkgs.sort_unstable_by(|a, b| a.name.cmp(&b.name));

    for pkg in pkgs {
        println!("{}", pkg.name);
    }

    Ok(())
}

fn pull(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
    package_names: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let repo_path = get_repo_path(proj_dirs, repos)?;
    if !repo_path.exists() {
        std::fs::create_dir_all(repo_path.as_ref())?;
    }

    let mut join_handles = vec![];

    for package_name in package_names {
        let package_name = package_name.clone();
        let path_base = repo_path.clone();
        join_handles.push(thread::spawn(move || {
            match pull_package(&path_base, &package_name) {
                Ok(_) => {}
                Err(e) => eprintln!("Error while pulling package: {:?}", e),
            }
        }));
    }

    for join_handle in join_handles {
        match join_handle.join() {
            Ok(_) => {}
            Err(e) => eprintln!("Failed to join thread: {:?}", e),
        };
    }

    Ok(())
}

fn pull_package(repo_path: &Path, package_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let full_path = repo_path.join(package_name);

    let repo = Repository::open(full_path)?;

    let mut remote = repo.find_remote(&"origin")?;
    remote.fetch(&["master"], None, None)?;

    let fetch_head = repo.find_reference("FETCH_HEAD")?;
    let fetch_commit = repo.reference_to_annotated_commit(&fetch_head)?;

    let mut refs_heads_master = repo.find_reference("refs/heads/master")?;

    let name = match refs_heads_master.name() {
        Some(name) => name.to_string(),
        None => String::from_utf8_lossy(refs_heads_master.name_bytes()).to_string(),
    };

    // TODO: Output which commits were added

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

fn get_repo_path(
    proj_dirs: ProjectDirs,
    repos: Option<PathBuf>,
) -> Result<Box<PathBuf>, Box<dyn std::error::Error>> {
    match repos {
        Some(s) => Ok(Box::new(s)),
        None => Ok(Box::new(proj_dirs.data_dir().join("repos").to_path_buf())),
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

fn check_repo_updates(
    dir: OsString,
    path_base: PathBuf,
    tx: mpsc::Sender<UpdateInfo>,
) -> Result<(), Box<dyn std::error::Error>> {
    let dir_name = String::from(dir.to_string_lossy());
    let full_path = path_base.join(dir);

    let repo = Repository::open(full_path)?;
    let mut remote = repo.find_remote(&"origin")?;
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

        tx.send(UpdateInfo {
            name: dir_name,
            commits,
        })?;
    }

    Ok(())
}

fn get_dir_list(pathbuf: &PathBuf) -> Result<Vec<OsString>, Error> {
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
