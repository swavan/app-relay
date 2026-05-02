use std::io::ErrorKind;
use std::net::TcpListener;
use std::path::PathBuf;

use apprelay_core::{
    ConfigStoreError, EventSink, FileEventSink, FileServerConfigRepository, InMemoryEventSink,
    ServerConfig, ServerConfigRepository,
};
use apprelay_protocol::Platform;
use apprelay_server::{
    DaemonServiceInstaller, ForegroundControlServer, ServerControlPlane, ServerServices,
};

fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let executable_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from(&args[0]));

    match args.get(1).map(String::as_str) {
        Some("service-plan") => {
            let platform = args.get(2).and_then(|value| parse_platform(value));
            print_service_plan(&executable_path, platform);
        }
        Some("install-service") => {
            let platform = args.get(2).and_then(|value| parse_platform(value));
            install_service_manifest(&executable_path, platform);
        }
        Some("uninstall-service-plan") => {
            let platform = args.get(2).and_then(|value| parse_platform(value));
            print_uninstall_service_plan(&executable_path, platform);
        }
        Some("uninstall-service") => {
            let platform = args.get(2).and_then(|value| parse_platform(value));
            write_uninstall_service_manifest(&executable_path, platform);
        }
        _ => run_foreground(&args),
    }
}

fn run_foreground(args: &[String]) {
    let log_path = option_value(args, "--log").map(PathBuf::from);
    let server = build_foreground_server(args).expect("failed to load server config");
    let bind_address = server.bind_address();
    let listener = TcpListener::bind(&bind_address).expect("failed to bind control listener");

    println!("apprelay-server listening on {bind_address}");
    match log_path {
        Some(path) => {
            let mut events = FileEventSink::new(path);
            run_once(&server, listener, &mut events);
        }
        None => {
            let mut events = InMemoryEventSink::default();
            run_once(&server, listener, &mut events);
        }
    }
}

fn build_foreground_server(args: &[String]) -> Result<ForegroundControlServer, ConfigStoreError> {
    let config_path = option_value(args, "--config").map(PathBuf::from);
    let services = ServerServices::for_current_platform();
    let control_plane = match config_path {
        Some(config_path) => {
            let repository = FileServerConfigRepository::new(config_path);
            let config = load_config(&repository)?;
            ServerControlPlane::with_config_repository(services, config, repository)
        }
        None => ServerControlPlane::new(services, ServerConfig::local("local-dev-token")),
    };

    Ok(ForegroundControlServer::new(control_plane))
}

fn run_once(server: &ForegroundControlServer, listener: TcpListener, events: &mut impl EventSink) {
    server
        .run_once(listener, events)
        .expect("control listener failed");
}

fn load_config(repository: &FileServerConfigRepository) -> Result<ServerConfig, ConfigStoreError> {
    match repository.load() {
        Ok(config) => Ok(config),
        Err(ConfigStoreError::Io(error)) if error.kind() == ErrorKind::NotFound => {
            let config = ServerConfig::local("local-dev-token");
            repository.save(&config)?;
            Ok(config)
        }
        Err(error) => Err(error),
    }
}

fn print_service_plan(executable_path: &PathBuf, platform: Option<Platform>) {
    let installer = DaemonServiceInstaller::new(executable_path);
    let plan = platform
        .map(|platform| installer.plan_for_platform(platform))
        .unwrap_or_else(|| installer.plan_for_current_platform())
        .expect("failed to build service plan");

    println!("manifest: {}", plan.manifest_path.display());
    println!("config: {}", plan.config_path.display());
    println!("log: {}", plan.log_path.display());
    println!("start: {}", plan.start_command);
    println!("stop: {}", plan.stop_command);
    println!("status: {}", plan.status_command);
    println!("uninstall: {}", plan.uninstall_command);
    println!();
    print!("{}", plan.manifest_contents);
}

fn install_service_manifest(executable_path: &PathBuf, platform: Option<Platform>) {
    let installer = DaemonServiceInstaller::new(executable_path);
    let plan = platform
        .map(|platform| installer.plan_for_platform(platform))
        .unwrap_or_else(|| installer.plan_for_current_platform())
        .expect("failed to build service plan");

    installer
        .install_manifest(&plan)
        .expect("failed to write service manifest");

    println!("wrote {}", plan.manifest_path.display());
    println!("start: {}", plan.start_command);
    println!("status: {}", plan.status_command);
}

fn print_uninstall_service_plan(executable_path: &PathBuf, platform: Option<Platform>) {
    let installer = DaemonServiceInstaller::new(executable_path);
    let plan = platform
        .map(|platform| installer.uninstall_plan_for_platform(platform))
        .unwrap_or_else(|| installer.uninstall_plan_for_current_platform())
        .expect("failed to build service uninstall plan");

    println!("manifest: {}", plan.manifest_path.display());
    println!("service-manifest: {}", plan.service_manifest_path.display());
    println!("run: {}", plan.run_command);
    println!();
    print!("{}", plan.manifest_contents);
}

fn write_uninstall_service_manifest(executable_path: &PathBuf, platform: Option<Platform>) {
    let installer = DaemonServiceInstaller::new(executable_path);
    let plan = platform
        .map(|platform| installer.uninstall_plan_for_platform(platform))
        .unwrap_or_else(|| installer.uninstall_plan_for_current_platform())
        .expect("failed to build service uninstall plan");

    installer
        .write_uninstall_manifest(&plan)
        .expect("failed to write service uninstall manifest");

    println!("wrote {}", plan.manifest_path.display());
    println!("run: {}", plan.run_command);
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == name).then(|| window[1].clone()))
}

fn parse_platform(value: &str) -> Option<Platform> {
    match value {
        "linux" => Some(Platform::Linux),
        "macos" => Some(Platform::Macos),
        "windows" => Some(Platform::Windows),
        "ios" => Some(Platform::Ios),
        "android" => Some(Platform::Android),
        "unknown" => Some(Platform::Unknown),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use apprelay_core::AuthorizedClient;

    #[test]
    fn foreground_config_mode_persists_pairing_revoke() {
        let root = unique_test_dir("foreground-config-revoke");
        let config_path = root.join("server.conf");
        let repository = FileServerConfigRepository::new(&config_path);
        let mut config = ServerConfig::local("correct-token");
        config.authorized_clients = vec![AuthorizedClient::new("test-client", "Test Client")];
        repository.save(&config).expect("save server config");
        let args = vec![
            "apprelay-server".to_string(),
            "--config".to_string(),
            config_path.display().to_string(),
        ];
        let server = build_foreground_server(&args).expect("build foreground server");
        let mut events = InMemoryEventSink::default();

        assert_eq!(
            server.handle_request("pairing-revoke correct-token test-client", &mut events),
            "OK pairing-revoke client_id=test-client label=Test%20Client"
        );
        assert!(repository
            .load()
            .expect("load persisted config")
            .authorized_clients
            .is_empty());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn load_config_rejects_corrupted_existing_config() {
        let root = unique_test_dir("foreground-config-corrupt");
        let config_path = root.join("server.conf");
        std::fs::create_dir_all(&root).expect("create config dir");
        std::fs::write(&config_path, "bad config").expect("write corrupt config");
        let repository = FileServerConfigRepository::new(&config_path);

        assert_eq!(
            load_config(&repository),
            Err(ConfigStoreError::CorruptedStore)
        );
        assert_eq!(
            std::fs::read_to_string(&config_path).expect("read corrupt config"),
            "bad config"
        );

        let _ = std::fs::remove_dir_all(root);
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after unix epoch")
            .as_nanos();

        std::env::temp_dir().join(format!("{name}-{}-{nanos}", std::process::id()))
    }
}
