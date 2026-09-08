#[derive(Clone, Debug)]
pub struct CollectorConfig {
    pub mode: String,
    pub data_dir: String,
}

impl CollectorConfig {
    pub fn load() -> Self {
        Self {
            mode: std::env::var("PUMPHOUND_MODE").unwrap_or_else(|_| "LIVE_WATCH".into()),
            data_dir: std::env::var("PUMPHOUND_DATA_DIR").unwrap_or_else(|_| "./data".into()),
        }
    }
}
