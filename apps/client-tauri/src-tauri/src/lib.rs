#[tauri::command]
fn server_health() -> String {
    "unconnected".to_string()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![server_health])
        .run(tauri::generate_context!())
        .expect("failed to run Swavan AppRelay client");
}
