use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::{env, fs, io};

fn main() -> ExitCode {
	let result = match env::args().nth(1).as_deref() {
		Some("setup") => setup(),
		Some("fmt") => fmt(false),
		Some("lint") => lint(),
		Some("doc") => doc(),
		Some("test") => test(),
		Some("msrv") => msrv(),
		Some("ci") => ci(),
		Some(command) => Err(format!("unknown command: {command}\n\n{USAGE}")),
		None => Err(USAGE.to_owned()),
	};
	match result {
		Ok(()) => ExitCode::SUCCESS,
		Err(message) => {
			eprintln!("{message}");
			ExitCode::FAILURE
		}
	}
}

const USAGE: &str = "Usage: cargo xtask <command>

Commands:
	setup  Link .claude to .agents
	fmt    Format the workspace
	lint   Run clippy on all targets and features, denying warnings
	doc    Build docs for all features, denying warnings, and compile doc examples
	test   Run the tests in a headless browser
	msrv   Check all targets and features with the rust-version in Cargo.toml
	ci     Check formatting, then lint, doc, msrv, and test, as CI does";

fn workspace_root() -> PathBuf {
	Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().to_owned()
}

fn fmt(check: bool) -> Result<(), String> {
	let mut args = vec!["fmt", "--all"];
	if check {
		args.push("--check");
	}
	cargo(&args, &[])
}

fn lint() -> Result<(), String> {
	cargo(
		&[
			"clippy",
			"--workspace",
			"--all-features",
			"--all-targets",
			"--",
			"-D",
			"warnings",
		],
		&[],
	)
}

fn doc() -> Result<(), String> {
	cargo(
		&["doc", "--workspace", "--all-features", "--no-deps"],
		&[("RUSTDOCFLAGS", "-D warnings")],
	)?;
	cargo(
		&["test", "--doc", "--workspace", "--all-features", "--target", "host-tuple"],
		&[],
	)
}

fn test() -> Result<(), String> {
	cargo(&["test", "--workspace", "--all-features"], &[])
}

fn ci() -> Result<(), String> {
	fmt(true)?;
	lint()?;
	doc()?;
	msrv()?;
	test()
}

fn msrv() -> Result<(), String> {
	let version = rust_version()?;
	let args = [
		"run",
		&version,
		"cargo",
		"check",
		"--workspace",
		"--all-features",
		"--all-targets",
		"--target",
		"wasm32-unknown-unknown",
	];
	run("rustup", &args, &[])
}

fn rust_version() -> Result<String, String> {
	let output = Command::new(cargo_program())
		.args(["metadata", "--no-deps", "--format-version", "1"])
		.current_dir(workspace_root())
		.output()
		.map_err(|error| format!("failed to run cargo metadata: {error}"))?;
	if !output.status.success() {
		return Err(format!("cargo metadata failed with {}", output.status));
	}
	let metadata: serde_json::Value = serde_json::from_slice(&output.stdout)
		.map_err(|error| format!("failed to parse cargo metadata: {error}"))?;
	metadata["packages"]
		.as_array()
		.into_iter()
		.flatten()
		.find(|package| package["name"] == "pka-webarrow")
		.and_then(|package| package["rust_version"].as_str())
		.map(str::to_owned)
		.ok_or_else(|| "pka-webarrow has no rust-version in Cargo.toml".to_owned())
}

fn cargo_program() -> String {
	env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned())
}

fn cargo(args: &[&str], envs: &[(&str, &str)]) -> Result<(), String> {
	run(&cargo_program(), args, envs)
}

fn run(program: &str, args: &[&str], envs: &[(&str, &str)]) -> Result<(), String> {
	let status = Command::new(program)
		.args(args)
		.envs(envs.iter().copied())
		.current_dir(workspace_root())
		.status()
		.map_err(|error| format!("failed to run {program}: {error}"))?;
	if status.success() {
		Ok(())
	} else {
		Err(format!("{program} {} failed with {status}", args.join(" ")))
	}
}

fn setup() -> Result<(), String> {
	let target = Path::new(".agents");
	let link = workspace_root().join(".claude");

	match fs::read_link(&link) {
		Ok(existing) if existing == target => {
			println!("{} -> {} already exists", link.display(), target.display());
			return Ok(());
		}
		Ok(existing) => {
			return Err(format!(
				"{} already points to {}",
				link.display(),
				existing.display()
			));
		}
		Err(error) if error.kind() == io::ErrorKind::NotFound => {}
		Err(_) => {
			return Err(format!("{} already exists and is not a symlink", link.display()));
		}
	}

	symlink_dir(target, &link)
		.map_err(|error| format!("failed to create {}: {error}", link.display()))?;
	println!("created {} -> {}", link.display(), target.display());
	Ok(())
}

#[cfg(unix)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
	std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn symlink_dir(target: &Path, link: &Path) -> io::Result<()> {
	std::os::windows::fs::symlink_dir(target, link)
}

#[cfg(not(any(unix, windows)))]
fn symlink_dir(_target: &Path, _link: &Path) -> io::Result<()> {
	Err(io::ErrorKind::Unsupported.into())
}
