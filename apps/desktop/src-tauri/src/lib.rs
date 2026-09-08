use keyring::Entry;

const SERVICE: &str = "com.pumphound.desktop";

fn allowed_secret_name(name: &str) -> bool {
    matches!(name, "helius" | "birdeye" | "pyth" | "siliconflow")
}

/// Persist a provider secret in the OS credential store. The value is never returned.
#[tauri::command]
fn store_provider_secret(name: String, value: String) -> Result<(), String> {
    if !allowed_secret_name(&name) {
        return Err("unsupported secret name".into());
    }
    if value.trim().is_empty() {
        return Err("empty secret".into());
    }
    let entry = Entry::new(SERVICE, &name).map_err(|e| e.to_string())?;
    entry.set_password(&value).map_err(|e| e.to_string())?;
    Ok(())
}

/// Report whether a named secret exists. Never echoes the secret.
#[tauri::command]
fn provider_secret_present(name: String) -> Result<bool, String> {
    if !allowed_secret_name(&name) {
        return Err("unsupported secret name".into());
    }
    let entry = Entry::new(SERVICE, &name).map_err(|e| e.to_string())?;
    Ok(entry.get_password().is_ok())
}

#[tauri::command]
fn clear_provider_secret(name: String) -> Result<(), String> {
    if !allowed_secret_name(&name) {
        return Err("unsupported secret name".into());
    }
    let entry = Entry::new(SERVICE, &name).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            store_provider_secret,
            provider_secret_present,
            clear_provider_secret
        ])
        .run(tauri::generate_context!())
        .expect("error while running PumpHound");
}
