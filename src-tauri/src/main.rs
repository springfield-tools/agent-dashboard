fn main() {
    let fixture_mode = std::env::args().any(|a| a == "--fixture");
    agent_dashboard_lib::run(fixture_mode);
}
