use std::time::Duration;
use std::path::{PathBuf, Path};
use std::fs::File;
use std::sync::Arc;

use callback::Callback;

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,

    pub file_read_started_callback:    Option<Arc<Callback<Path, File>>>,
    pub file_write_started_callback:   Option<Arc<Callback<Path, File>>>,
    pub file_read_completed_callback:  Option<Arc<Callback<Path, File>>>,
    pub file_write_completed_callback: Option<Arc<Callback<Path, File>>>,

    pub read_timeout: Option<Duration>,
    pub send_retry_attempts: u8
}
