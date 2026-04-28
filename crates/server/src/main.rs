use swavan_server::ServerServices;

fn main() {
    let services = ServerServices::for_current_platform();
    let health = services.health();

    println!(
        "{} {} healthy={}",
        health.service, health.version, health.healthy
    );
}
