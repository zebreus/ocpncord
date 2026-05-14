use opencode_backend_opencode::OpenCodeBackend;
use opencode_tui::App;

#[tokio::main]
async fn main() {
    let backend = OpenCodeBackend::new_std("http://localhost:4096");
    let _app = App::new(backend);

    println!("opencode-native starting...");
}
