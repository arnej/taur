taur
====

**taur** (Tiny AUR helper) is a utility for managing Arch Linux AUR repositories. It is intended for users that manually build AUR packages with `makepkg`, but don't want to manually check and update every single git repository.

This project was born when my shell script for checking for updates in AUR packages became more and more difficult to maintain and extend. To be able to provide more functionality and a code base that's easier to maintain, I decided to rebuild the same functionality in the [Rust programming language][rust-lang]. After a few hours, **taur** had the same functionality as my previous shell script and quickly advanced with more and more features.

Features
--------

- Fetch all local AUR repositories and print available updates (new commits inside the remote repository)
- Pull all or some local AUR repositories
- Search for packages in AUR
- Clone new packages from AUR
- Fetch and pull are done in parallel for all specified repositories

Installation
------------

Currently, the only option is using cargo. Other installation options may be added later when needed.
Install **taur** by running the following command:

```sh
cargo install taur
```

Usage
-----

| Command | Function |
| ------- | -------- |
| `taur` | Same as `taur fetch` |
| `taur clone` <package_name> | Clone a package with the given name from AUR |
| `taur fetch` | Fetch all local repositories and print new commits |
| `taur pull <package_names>` | Pull given package repositories (or all when no package is specified) |
| `taur search <expression>` | Search AUR packages by specified expression |

Status
------

**taur** works for my current needs and does so very fast. There is room for improvement though (like using a thread pool and better structuring the code).

Also, new functionality is planned and proposals and pull requests are *very* welcome.
