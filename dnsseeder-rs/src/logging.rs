use log::LevelFilter;

pub fn init(
    no_log_files: bool,
    log_level: &str,
    log_file: &str,
    err_log_file: &str,
) -> Result<(), String> {
    let level = level_from_string(log_level)?;
    let mut dispatch = fern::Dispatch::new().format(|out, message, record| {
        let ts = time::OffsetDateTime::now_utc()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_else(|_| "0001-01-01T00:00:00Z".to_string());
        out.finish(format_args!("{ts} [{}] {}", record.level(), message))
    });

    dispatch = dispatch.chain(fern::Dispatch::new().level(level).chain(std::io::stdout()));

    if !no_log_files {
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .level(LevelFilter::Trace)
                .chain(fern::log_file(log_file).map_err(|e| e.to_string())?),
        );
        dispatch = dispatch.chain(
            fern::Dispatch::new()
                .level(LevelFilter::Warn)
                .chain(fern::log_file(err_log_file).map_err(|e| e.to_string())?),
        );
    }

    dispatch.apply().map_err(|e| e.to_string())?;
    Ok(())
}

fn level_from_string(level: &str) -> Result<LevelFilter, String> {
    match level.to_lowercase().as_str() {
        "trace" => Ok(LevelFilter::Trace),
        "debug" => Ok(LevelFilter::Debug),
        "info" => Ok(LevelFilter::Info),
        "warn" | "warning" => Ok(LevelFilter::Warn),
        "error" => Ok(LevelFilter::Error),
        "critical" => Ok(LevelFilter::Error),
        "off" => Ok(LevelFilter::Off),
        _ => Err(format!("Invalid loglevel: {}", level)),
    }
}
