use directories::ProjectDirs;
use git2::Repository;
use std::ffi::OsString;
use std::io::Error;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use structopt::StructOpt;
use termion::{color, style};

#[derive(Debug, StructOpt)]
#[structopt(name = "taur", about = "Tiny AUR helper")]
struct Args {
    #[structopt()]
    repos: Option<PathBuf>,
    #[structopt(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, StructOpt)]
enum Command {
    #[structopt(name = "fetch")]
    Fetch,
    #[structopt(name = "search")]
    Search { expression: String },
    #[structopt(name = "pull")]
    Pull { package_names: Vec<String> },
}

#[derive(Eq, Ord)]
struct UpdateInfo {
    name: String,
    commits: Vec<String>,
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
            Command::Fetch => fetch(proj_dirs, args.repos)?,
            Command::Pull { package_names } => pull(proj_dirs, args.repos, package_names)?,
            Command::Search { expression } => search(expression)?,
        },
        None => fetch(proj_dirs, args.repos)?,
    }

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
        join_handles.push(thread::spawn(
            move || match check_repo_updates(dir, path_base.to_path_buf(), tx) {
                Ok(_) => {}
                Err(e) => eprintln!("Error while checking for updates for repo {:?}", e),
            },
        ));
    }

    // Drop tx to get rid of the original unused sender
    drop(tx);

    for join_handle in join_handles {
        match join_handle.join() {
            Ok(_) => {},
            Err(e) => eprintln!("Failed to join thread: {:?}", e)
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

    for package_name in package_names {
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
    }

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
        println!();
        println!(
            "{}The following packages have upstream changes:{}",
            style::Bold,
            style::Reset
        );
        println!();

        update_infos.sort_unstable();

        for info in update_infos {
            println!(
                "{}{}:: {}{}{}",
                style::Bold,
                color::Fg(color::Blue),
                color::Fg(color::Reset),
                info.name,
                style::Reset
            );
            println!();

            for commit in info.commits {
                println!(
                    "{}* {}{}{}",
                    color::Fg(color::Magenta),
                    color::Fg(color::Cyan),
                    commit,
                    style::Reset
                );
            }
        }
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
                // TODO: Remove all empty lines
                commits.push(c.replace("\n\n", "\n"));
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
