use std::net::TcpListener;

use swavan_core::{InMemoryEventSink, ServerConfig};
use swavan_server::{ForegroundControlServer, ServerControlPlane, ServerServices};

fn main() {
    let config = ServerConfig::local("local-dev-token");
    let services = ServerServices::for_current_platform();
    let control_plane = ServerControlPlane::new(services, config);
    let server = ForegroundControlServer::new(control_plane);
    let bind_address = server.bind_address();
    let listener = TcpListener::bind(&bind_address).expect("failed to bind control listener");
    let mut events = InMemoryEventSink::default();

    println!("swavan-server listening on {bind_address}");
    server
        .run_once(listener, &mut events)
        .expect("control listener failed");
}
