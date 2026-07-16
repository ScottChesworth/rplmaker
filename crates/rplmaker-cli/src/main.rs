use clap::Parser;
use std::path::PathBuf;
use std::process::ExitCode;

/// Convert plugin preset files into a REAPER preset library (RPL).
///
/// The template is an RPL exported from REAPER that contains at least one
/// preset saved natively for the target plugin; it teaches rplmaker the
/// plugin-specific wrapper. Save any preset through REAPER's FX preset
/// combobox, then export it via the preset menu's "Export preset library"
/// to create one.
#[derive(Parser)]
#[command(name = "rplmaker", version)]
struct Args {
    /// Template RPL exported from REAPER for the target plugin
    #[arg(short, long)]
    template: PathBuf,

    /// Output RPL file to create
    #[arg(short, long)]
    output: PathBuf,

    /// Prepend a folder marker to the first preset of each subfolder, so
    /// folder changes are announced while arrowing through REAPER's flat
    /// preset list. "deepest" uses the innermost folder name ("Adam
    /// Christianson folder: Dreamolo"); "full" uses the whole relative path
    /// ("Artists, Adam Christianson folder: Dreamolo"). Bare
    /// --folder-markers means "deepest"
    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "deepest")]
    folder_markers: Option<MarkerStyle>,

    /// Also scan a plugin binary (.vst3, .dll) for embedded factory
    /// presets and convert whatever is found, with positional names
    #[arg(long)]
    scan_plugin: Option<PathBuf>,

    /// Preset files or folders to convert
    presets: Vec<PathBuf>,
}

#[derive(clap::ValueEnum, Clone, Copy)]
enum MarkerStyle {
    Deepest,
    Full,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match run(&args) {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &Args) -> Result<String, Box<dyn std::error::Error>> {
    let template_text = std::fs::read_to_string(&args.template)
        .map_err(|e| format!("cannot read template {}: {e}", args.template.display()))?;
    let template = rplmaker_core::load_template(&template_text)?;
    eprintln!("template library: {}", template.library_header);

    let mut inputs = args.presets.clone();
    if let Some(plugin) = &args.scan_plugin {
        let binary = std::fs::read(plugin)
            .map_err(|e| format!("cannot read plugin {}: {e}", plugin.display()))?;
        let docs = rplmaker_core::extract::extract_embedded_presets(&template, &binary);
        if docs.is_empty() {
            eprintln!(
                "no embedded presets found in {} (looked for '{}' state documents; \
                 the plugin may compress its resources)",
                plugin.display(),
                template.state_root_name()
            );
        } else {
            let stem = plugin
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "Plugin".to_string());
            let dir = std::env::temp_dir().join(format!("rplmaker-extracted-{stem}"));
            rplmaker_core::extract::write_extracted(&dir, &stem, &docs)?;
            eprintln!("extracted {} embedded preset(s) from {}", docs.len(), plugin.display());
            inputs.push(dir);
        }
    }
    if inputs.is_empty() {
        return Err("give preset files or folders to convert, or --scan-plugin".into());
    }

    let files = rplmaker_core::files::collect_preset_files(&inputs)?;
    if files.is_empty() {
        return Err("no preset files found in the given paths".into());
    }

    let naming = match args.folder_markers {
        None => rplmaker_core::FolderNaming::Flat,
        Some(MarkerStyle::Deepest) => rplmaker_core::FolderNaming::Deepest,
        Some(MarkerStyle::Full) => rplmaker_core::FolderNaming::FullPath,
    };
    let outcome = rplmaker_core::convert_files(&template, &files, naming);
    for preset in &outcome.presets {
        eprintln!("converted: {} ({} parameters)", preset.name, preset.parameters_applied);
    }
    for (path, e) in &outcome.skipped {
        eprintln!("skipped {}: {e}", path.display());
    }
    if outcome.presets.is_empty() {
        return Err("no presets could be converted".into());
    }

    let out_text = rplmaker_core::build_rpl(&template, &outcome.presets);
    std::fs::write(&args.output, out_text)
        .map_err(|e| format!("cannot write {}: {e}", args.output.display()))?;

    Ok(format!(
        "Wrote {} preset(s) to {}{}. In REAPER, use the FX preset menu's \"Import preset library\" to load it.",
        outcome.presets.len(),
        args.output.display(),
        if outcome.skipped.is_empty() {
            String::new()
        } else {
            format!(", {} skipped", outcome.skipped.len())
        }
    ))
}
