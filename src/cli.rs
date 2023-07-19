use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Args {
    /// Use a different output path than in the config
    #[arg(short)]
    pub output: Option<String>,

    /// Skip downloading existing files
    #[arg(short)]
    pub skip_existing: bool,

    /// Encoding profile from the config to use
    #[arg(short)]
    pub encoding_profile: Option<String>,

    /// Spotify URI/URL or the resource that you want to download (track, album, playlist, etc.)
    pub resource: String,
}
