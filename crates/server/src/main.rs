use std::net::TcpListener;
use std::path::PathBuf;

use apprelay_core::{
    EventSink, FileEventSink, FileServerConfigRepository, InMemoryEventSink, ServerConfig,
    ServerConfigRepository,
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
    let config_path = option_value(args, "--config").map(PathBuf::from);
    let log_path = option_value(args, "--log").map(PathBuf::from);
    let config = load_config(config_path.as_ref());
    let services = ServerServices::for_current_platform();
    let control_plane = ServerControlPlane::new(services, config);
    let server = ForegroundControlServer::new(control_plane);
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

fn run_once(server: &ForegroundControlServer, listener: TcpListener, events: &mut impl EventSink) {
    server
        .run_once(listener, events)
        .expect("control listener failed");
}

fn load_config(config_path: Option<&PathBuf>) -> ServerConfig {
    let Some(config_path) = config_path else {
        return ServerConfig::local("local-dev-token");
    };
    let repository = FileServerConfigRepository::new(config_path);

    match repository.load() {
        Ok(config) => config,
        Err(_) => {
            let config = ServerConfig::local("local-dev-token");
            repository
                .save(&config)
                .expect("failed to initialize server config");
            config
        }
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
