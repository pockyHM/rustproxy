use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::time::Duration;

pub fn watch_config<F>(path: &str, on_change: F) -> anyhow::Result<RecommendedWatcher>
where
    F: Fn() + Send + 'static,
{
    let (tx, rx) = channel();

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<notify::Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        },
        Config::default().with_poll_interval(Duration::from_secs(1)),
    )?;

    let path = Path::new(path);
    if let Some(parent) = path.parent() {
        watcher.watch(parent, RecursiveMode::NonRecursive)?;
    }
    watcher.watch(path, RecursiveMode::NonRecursive)?;

    // Spawn a thread to handle events and call on_change
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            if event.kind.is_modify() {
                on_change();
            }
        }
    });

    Ok(watcher)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tempfile::NamedTempFile;

    #[test]
    fn test_watch_config_creates_watcher() {
        let yaml_content = r#"
version: "1.0"
listen: "0.0.0.0:8080"
rules: []
upstreams: {}
fallback:
  url: "http://fallback.example.com"
"#;
        let file = create_temp_file(yaml_content);

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let result = watch_config(file.path().to_str().unwrap(), move || {
            called_clone.store(true, Ordering::SeqCst);
        });

        assert!(result.is_ok());
        // Watcher is created successfully
    }

    #[test]
    fn test_watch_config_nonexistent_path() {
        let result = watch_config("/nonexistent/path/config.yaml", || {});
        assert!(result.is_err());
    }

    fn create_temp_file(content: &str) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }
}
