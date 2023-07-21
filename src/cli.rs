use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Args {
    /// Use a different output path than in the config
    #[arg(short, long)]
    pub output: Option<String>,

    /// Skip downloading existing files
    #[arg(short, long)]
    pub skip_existing: bool,

    /// Encoding profile from the config to use
    #[arg(short, long)]
    pub encoding_profile: Option<String>,

    // Save the cover art of the first track in a directory as a file with the given name (relative to the track directory)
    #[arg(long)]
    pub external_cover_art: Option<String>,

    /// Spotify URI/URL or the resource that you want to download (track, album, playlist, etc.)
    pub resource: String,
}
