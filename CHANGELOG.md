## Next Version

## v0.2.0 - 2023-07-15
### Fixed
- Fixed asynchronous handling of git commands (this is also a nice performance boost)

## v0.1.6 - 2020-11-24
### Fixed
- Forgot updating Cargo.lock

## v0.1.5 - 2020-11-24
### Added
- Updated raur to 0.4, now using its async functions

### Fixed
- Ord and PartialOrd for UpdateInfo now agree with each other

## v0.1.4 - 2020-06-23
### Added
- Switched from manually spawning threads to using tokio

## v0.1.3 - 2019-11-12
### Changed
- Provide better error messages

### Fixed
- Check if a repository exists before trying to clone

## v0.1.2 - 2019-10-28
### Added
- Print new commits when pulling repositories

## v0.1.1 - 2019-10-22
- Initial public release
