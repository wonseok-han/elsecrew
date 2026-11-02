//! UI와 데스크톱 런타임에 독립적인 Hello, Agent 안전 코어.

// Protocol fixtures need host-absolute paths even when no native provider is run.
// Keep production path validation identical on every target.
#[cfg(test)]
macro_rules! test_path {
    ($base:literal, $child:literal) => {
        std::path::Path::new(test_path!($base))
            .join($child)
            .to_string_lossy()
            .into_owned()
    };
    ($path:literal) => {{
        #[cfg(windows)]
        const PATH: &str = concat!("C:", $path);
        #[cfg(not(windows))]
        const PATH: &str = $path;
        PATH
    }};
}

pub mod agent;
pub mod agent_adapter;
pub mod claude_pilot;
pub mod codex_pilot;
pub mod error;
pub mod install_lifecycle;
pub mod proctree;
pub mod provider;
