use std::collections::HashSet;
use std::sync::{OnceLock, RwLock};

static HOT_MINTS: OnceLock<RwLock<HashSet<String>>> = OnceLock::new();

fn store() -> &'static RwLock<HashSet<String>> {
    HOT_MINTS.get_or_init(|| RwLock::new(HashSet::new()))
}

pub fn replace_hot_mints(mints: impl IntoIterator<Item = String>) {
    if let Ok(mut guard) = store().write() {
        *guard = mints.into_iter().filter(|mint| mint.len() >= 32).collect();
    }
}

pub fn is_hot_mint(mint: &str) -> bool {
    store()
        .read()
        .map(|guard| guard.contains(mint))
        .unwrap_or(false)
}

pub fn logs_mention_hot(logs: &[String]) -> bool {
    let Ok(guard) = store().read() else {
        return false;
    };
    if guard.is_empty() {
        return false;
    }
    logs.iter()
        .any(|line| guard.iter().any(|mint| line.contains(mint.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hot_mint_in_logs_is_detected() {
        replace_hot_mints([
            "So11111111111111111111111111111111111111112".into(),
            "short".into(),
        ]);
        assert!(logs_mention_hot(&[
            "Program log: mint So11111111111111111111111111111111111111112".into()
        ]));
        assert!(is_hot_mint("So11111111111111111111111111111111111111112"));
        assert!(!logs_mention_hot(&["Program log: unrelated".into()]));
    }
}
