#[cfg(windows)]
mod windows;
#[cfg(any(windows, test))]
mod windows_launch;
fn main() {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("check") {
        let path = args
            .get(2)
            .map(String::as_str)
            .unwrap_or("config/config.lua");
        let platform = args.get(3).map(String::as_str).unwrap_or(if cfg!(windows) {
            "windows"
        } else {
            "macos"
        });
        let result = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|s| qck::validate(&s, platform));
        match result {
            Ok(bindings) => println!("{}", serde_json::to_string_pretty(&bindings).unwrap()),
            Err(e) => {
                eprintln!("{e}");
                std::process::exit(1);
            }
        }
        return;
    }
    #[cfg(windows)]
    {
        if let Err(e) = windows::run(args.get(1).map(String::as_str)) {
            eprintln!("qck: {e}");
            std::process::exit(1);
        }
    }
    #[cfg(not(windows))]
    {
        eprintln!(
            "Build the macOS app with scripts/build-macos.sh. Validate config: cargo run -- check [path] [macos|windows]"
        );
    }
}
