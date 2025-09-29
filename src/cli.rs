use clap::Parser;

#[derive(Parser)]
#[command(
    name = "tmp",
    about = "Helper function to quickly make file types defined in the config file",
    version = env!("GIT_DESCRIBE"),
    author = "Scott Idler <scott.a.idler@gmail.com>",
    after_help = "Logs are written to: ~/.local/share/tmp/tmp.log"
)]
pub struct Cli {
    /// Config filepath
    #[arg(
        long,
        value_name = "FILEPATH",
        default_value = "~/.config/tmp/tmp.yml",
        help = "Config filepath"
    )]
    pub config: String,

    /// Only print contents of the file to be made
    #[arg(short = 'N', long, help = "Only print contents of the file to be made")]
    pub nerf: bool,

    /// Delete filename
    #[arg(short = 'r', long, help = "Delete filename")]
    pub rm: bool,

    /// Set the value to chmod the file to
    #[arg(short = 'c', long, value_name = "MODE", help = "Set the value to chmod the file to")]
    pub chmod: Option<String>,

    /// Choose which kind of tmp file
    #[arg(value_name = "KIND", help = "Choose which kind of tmp file")]
    pub kind: String,

    /// Optionally name the script
    #[arg(value_name = "NAME", help = "Optionally name the script")]
    pub name: Option<String>,
}
