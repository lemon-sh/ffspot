use std::{
    env, fs, io,
    path::{Path, PathBuf},
    process,
};

use clap::Parser;
use color_eyre::{
    eyre::{eyre, Context},
    Result,
};
use colored::Colorize;
use config::LoadResult;
use librespot::{
    core::{cache::Cache, config::SessionConfig, session::Session},
    discovery::Credentials,
};
use regex::Regex;
use tracing::Level;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

mod cli;
mod config;
mod download;
mod resolve;
mod template;

#[tokio::main]
async fn main() -> Result<()> {
    color_eyre::config::HookBuilder::default()
        .display_env_section(false)
        .install()?;

    #[cfg(windows)]
    colored::control::set_virtual_terminal(true).unwrap();

    if let Ok(log_location) = env::var("FFSPOT_LOG") {
        let path = PathBuf::from(log_location);
        let (dir, file): (&Path, PathBuf) = if path.is_dir() {
            (&path, path.join("ffspot.log"))
        } else {
            (path.parent().unwrap(), path.clone())
        };
        let logfile = RollingFileAppender::new(Rotation::NEVER, dir, file);
        tracing_subscriber::fmt()
            .with_writer(logfile)
            .with_max_level(Level::DEBUG)
            .init();
    }

    let cli = cli::Args::parse();

    let config = match config::load()? {
        LoadResult::Opened(c) => c,
        LoadResult::Created(path) => {
            eprintln!(
                "{} {path}\n{}",
                "A new configuration file has been created in".bright_green(),
                "Adjust it and run ffspot again.".bright_magenta()
            );
            return Ok(());
        }
    };

    ffmpeg_healthcheck(&config.ffpath)?;

    let Some((resource_type, resource_id)) = parse_spotify_uri(&cli.resource) else {
        eprintln!(
            "{}",
            "Error: The supplied resource URL/URI is invalid.".bright_red()
        );
        process::exit(2)
    };

    eprintln!("{}", "Logging in...".bright_cyan());

    let (session, username) = login(&config.username, &config.password)
        .await
        .wrap_err("Login failed. Make sure that the credentials in the config file are correct.")?;

    eprintln!("{}{}", "Logged in as ".bright_green(), username);

    download::download(resource_type, resource_id, session, config, &cli).await?;

    Ok(())
}

fn ffmpeg_healthcheck(ffpath: impl AsRef<Path>) -> Result<()> {
    let ffpath = ffpath.as_ref();
    if which::which(ffpath).is_err() {
        return Err(eyre!("{ffpath:?} binary not found. Make sure FFmpeg is installed, or if you set a custom ffmpeg path, that the path is correct."));
    }
    Ok(())
}

fn parse_spotify_uri(uri: &str) -> Option<(&str, &str)> {
    let regex = Regex::new(
        r"(?:https?|spotify):(?://open\.spotify\.com/)?(track|album|playlist)[/:]([a-zA-Z\d]*)",
    )
    .unwrap();
    let captures = regex.captures(uri)?;
    Some((captures.get(1)?.as_str(), captures.get(2)?.as_str()))
}

async fn login(
    username: impl Into<String>,
    password: impl Into<String>,
) -> Result<(Session, String)> {
    let credentials = Credentials::with_password(username, password);
    let username = credentials.username.clone();
    let cache = Cache::new(Some(get_credcache_path()?), None, None, None)?;
    let session = Session::new(SessionConfig::default(), Some(cache));
    Session::connect(&session, credentials, true).await?;
    Ok((session, username))
}

fn get_credcache_path() -> io::Result<PathBuf> {
    if let Ok(path) = env::var("FFSPOT_CREDCACHE") {
        return Ok(path.into());
    }

    let cachedir = if let Some(dir) = dirs::cache_dir() {
        dir
    } else {
        env::current_dir()?
    }
    .join("ffspot")
    .join("credentials_cache");

    if !cachedir.is_dir() {
        fs::create_dir_all(&cachedir)?;
    }

    Ok(cachedir)
}
