pub const BUILD_VERSION: &str = env!("RUSTPROXY_BUILD_VERSION");
pub const PACKAGE_VERSION: &str = env!("RUSTPROXY_PACKAGE_VERSION");
pub const GIT_REF: &str = env!("RUSTPROXY_GIT_REF");
pub const GIT_REF_KIND: &str = env!("RUSTPROXY_GIT_REF_KIND");
pub const GIT_COMMIT: &str = env!("RUSTPROXY_GIT_COMMIT");
pub const GIT_DIRTY: &str = env!("RUSTPROXY_GIT_DIRTY");

pub fn git_dirty() -> bool {
    GIT_DIRTY == "true"
}
