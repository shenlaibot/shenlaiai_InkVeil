// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
pub mod audit;
pub mod commands;
pub mod models;
pub mod sidecar;

use std::sync::Mutex;

use tauri::Manager;

#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // OTA 升级：endpoints/pubkey 在 tauri.conf.json 的 plugins.updater 中配置，
    // 发布时在流水线用 keys/updater.key 签名更新包。
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(models::DownloadManager::new())
        .manage(commands::VaultState(Mutex::new(None)))
        .setup(|app| {
            // 应用状态持有 sidecar 管理器句柄（随应用退出时 Drop 兜底终止进程）。
            app.manage(Mutex::new(None::<sidecar::SidecarManager>));

            // SQLite 审计日志与应用配置（应用数据目录，本地零外传）。
            if let Ok(data_dir) = app.path().app_data_dir() {
                if std::fs::create_dir_all(&data_dir).is_ok() {
                    match audit::AuditDb::open(&data_dir.join("anonkit.db")) {
                        Ok(db) => {
                            app.manage(db);
                            eprintln!(
                                "[audit] sqlite ready at {}",
                                data_dir.join("anonkit.db").display()
                            );
                        }
                        Err(e) => eprintln!("[audit] sqlite init failed (non-fatal): {e}"),
                    }
                }
            }

            // 开发模式启动 sidecar：失败仅告警，不阻塞应用启动。
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                match sidecar::SidecarManager::start().await {
                    Ok(manager) => {
                        eprintln!(
                            "[sidecar] ready at {} (health: {})",
                            manager.base_url(),
                            manager.health_url()
                        );
                        let state = handle.state::<Mutex<Option<sidecar::SidecarManager>>>();
                        let mut guard = match state.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        *guard = Some(manager);
                    }
                    Err(err) => {
                        eprintln!("[sidecar] failed to start (non-fatal): {err}");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            // 文件导入
            commands::pick_files,
            commands::pick_folder,
            commands::pick_mapping_file,
            commands::collect_files,
            commands::pick_output_dir,
            commands::open_in_folder,
            commands::ner_model_path,
            commands::save_text_file,
            // 扫描 / 脱敏 / 还原
            commands::get_builtin_rules,
            commands::scan_files,
            commands::confirm_redact,
            commands::restore_text,
            commands::restore_text_fuzzy,
            commands::pick_restore_doc,
            commands::restore_document,
            // 项目保险库
            commands::pick_vault_file,
            commands::vault_create,
            commands::vault_open,
            commands::vault_info,
            commands::vault_close,
            commands::vault_set_dict,
            // 模型市场
            models::list_models,
            models::model_status,
            models::model_install_dir,
            models::start_model_download,
            models::pause_model_download,
            models::delete_model,
            // 审计日志与应用配置（SQLite）
            audit::audit_list,
            audit::audit_add,
            audit::audit_verify,
            audit::audit_prune,
            audit::audit_retention_get,
            audit::audit_retention_set,
            audit::config_get,
            audit::config_set,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            // 应用退出时主动终止 sidecar，不依赖 Drop 是否被执行
            if let tauri::RunEvent::Exit = event {
                let state = app_handle.state::<Mutex<Option<sidecar::SidecarManager>>>();
                let mut guard = match state.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                if let Some(manager) = guard.take() {
                    drop(manager); // Drop 中 start_kill
                }
            }
        });
}
