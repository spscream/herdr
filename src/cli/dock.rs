pub(super) fn run_dock_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_dock_help();
        return Ok(2);
    };

    match subcommand {
        "toggle" => dock_toggle(&args[1..]),
        "--help" | "-h" | "help" => {
            print_dock_help();
            Ok(0)
        }
        other => {
            eprintln!("unknown dock subcommand: {other}");
            print_dock_help();
            Ok(2)
        }
    }
}

fn dock_toggle(args: &[String]) -> std::io::Result<i32> {
    if let Some(unexpected) = args.first() {
        eprintln!("dock toggle takes no arguments, got: {unexpected}");
        return Ok(2);
    }
    super::runtime::dock_toggle()
}

fn print_dock_help() {
    eprintln!("herdr dock commands:");
    eprintln!("  herdr dock toggle           collapse or restore the dock column");
}
