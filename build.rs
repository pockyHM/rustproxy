use std::{env, fs, process::Command};

fn main() {
    println!("cargo:rerun-if-env-changed=RUSTPROXY_VERSION");
    println!("cargo:rerun-if-env-changed=RUSTPROXY_GIT_REF");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/packed-refs");

    if let Ok(head) = fs::read_to_string(".git/HEAD") {
        if let Some(ref_path) = head.trim().strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=.git/{ref_path}");
        }
    }

    let package_version = env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".to_string());
    let commit = git(["rev-parse", "--short=12", "HEAD"]).unwrap_or_default();
    let dirty = git(["status", "--porcelain"])
        .map(|status| !status.trim().is_empty())
        .unwrap_or(false);

    let detected_ref = detect_git_ref();
    let ref_name = env::var("RUSTPROXY_GIT_REF")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| detected_ref.name.clone());

    let build_version = env::var("RUSTPROXY_VERSION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| match detected_ref.kind.as_str() {
            "tag" => append_dirty(&ref_name, dirty),
            "branch" if !commit.is_empty() => append_dirty(&format!("{ref_name}@{commit}"), dirty),
            "branch" => append_dirty(&ref_name, dirty),
            _ if !commit.is_empty() => append_dirty(&format!("{package_version}@{commit}"), dirty),
            _ => package_version.clone(),
        });

    println!("cargo:rustc-env=RUSTPROXY_BUILD_VERSION={build_version}");
    println!("cargo:rustc-env=RUSTPROXY_PACKAGE_VERSION={package_version}");
    println!("cargo:rustc-env=RUSTPROXY_GIT_REF={ref_name}");
    println!("cargo:rustc-env=RUSTPROXY_GIT_REF_KIND={}", detected_ref.kind);
    println!("cargo:rustc-env=RUSTPROXY_GIT_COMMIT={commit}");
    println!("cargo:rustc-env=RUSTPROXY_GIT_DIRTY={dirty}");
}

struct GitRef {
    kind: String,
    name: String,
}

fn detect_git_ref() -> GitRef {
    if let Some(tag) = git(["describe", "--tags", "--exact-match", "HEAD"]) {
        return GitRef {
            kind: "tag".to_string(),
            name: tag,
        };
    }

    if let Some(branch) = git(["rev-parse", "--abbrev-ref", "HEAD"]) {
        if branch != "HEAD" {
            return GitRef {
                kind: "branch".to_string(),
                name: branch,
            };
        }
    }

    GitRef {
        kind: "unknown".to_string(),
        name: "unknown".to_string(),
    }
}

fn append_dirty(value: &str, dirty: bool) -> String {
    if dirty {
        format!("{value}+dirty")
    } else {
        value.to_string()
    }
}

fn git<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let value = String::from_utf8(output.stdout).ok()?;
    let value = value.trim().to_string();
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}
