pub(super) fn run_dock_command(args: &[String]) -> std::io::Result<i32> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        print_dock_help();
        return Ok(2);
    };

    match subcommand {
        "toggle" => dock_toggle(&args[1..]),
        "get" => dock_target_command(&args[1..], "get", super::runtime::dock_get),
        "close" => dock_target_command(&args[1..], "close", super::runtime::dock_close),
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

/// Parse the one flag `get` and `close` share, then run the given verb.
fn dock_target_command(
    args: &[String],
    verb: &str,
    run: fn(crate::api::schema::DockTarget) -> std::io::Result<i32>,
) -> std::io::Result<i32> {
    let mut workspace_id = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--workspace" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("missing value for --workspace");
                    return Ok(2);
                };
                workspace_id = Some(super::normalize_workspace_id(value));
                index += 2;
            }
            other => {
                eprintln!("unknown option for dock {verb}: {other}");
                return Ok(2);
            }
        }
    }
    run(crate::api::schema::DockTarget { workspace_id })
}

fn print_dock_help() {
    eprintln!("herdr dock commands:");
    eprintln!("  herdr dock toggle           collapse or restore the dock column");
    eprintln!("  herdr dock get [--workspace <workspace_id>]");
    eprintln!("                              report the dock column of a workspace");
    eprintln!("  herdr dock close [--workspace <workspace_id>]");
    eprintln!("                              stop the process in the dock column");
}
